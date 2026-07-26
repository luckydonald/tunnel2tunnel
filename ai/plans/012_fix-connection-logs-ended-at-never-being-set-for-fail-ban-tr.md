# Fix `connection_logs.ended_at` never being set for Fail/Ban/Trap rows

## Context

`EntityDetailPage.vue`'s connection log table now shows an "Ended" column (commit `bdf7dc8`), but `ended_at` is always `null`. Investigation shows the backend only captures the created row's `id` on the **successful login** path (`log_auth_success` sets `self.log_id`), then `T2tHandler`'s `Drop` impl calls `ConnectionLog::set_ended` for that one id. Every other code path that creates a `connection_logs` row throws the id away, so `ended_at` can never be back-filled for it:

- **Fail** rows (`log_auth_failure`, called from `auth_none`/`auth_password`/`auth_keyboard_interactive`/`auth_publickey`) — id is never stored on `self`, so `Drop` never marks them ended.
- **Trap** rows created via the same `log_auth_failure` path (fake-shell, slow-auth) — same bug as Fail.
- **Trap** rows created by the banner-drip pre-auth tarpit (`tarpit/banner_drip.rs::run`) — this path is actually already correct: it holds the log id locally and calls `set_ended` once its drip loop exits.
- **Ban** rows created by the hard pre-auth ban (`tarpit::log_hard_ban` in `tarpit/mod.rs`) — this connection is rejected and closed synchronously in the accept loop, before any `T2tHandler`/`Drop` ever exists for it, so there is no mechanism at all to mark it ended.
- **OK** (successful login) rows — code review shows this path is already wired correctly (`log_id` captured, `Drop` spawns `set_ended`). This will be re-verified manually after the fix lands, since the user observed it as null too; if it turns out broken, the fix below (broadening to a `Vec<Uuid>`) covers this path as well without further change.

## Fix

All changes in `crates/tunnel2tunnel-ssh/src/lib.rs` and `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`.

1. **`T2tHandler`**: replace the single `log_id: Option<Uuid>` field with `connection_log_ids: Vec<Uuid>`, collecting the id of *every* `connection_logs` row created for this TCP connection (success and failure/trap alike — one connection can rack up several failed attempts before either succeeding or being dropped).

2. **`log_auth_success`** (currently `&mut self`): push the created row's id into `connection_log_ids` instead of overwriting `log_id`.

3. **`log_auth_failure`** (currently `&self`): change to `&mut self` and push the created row's id into `connection_log_ids` on success (mirrors `log_auth_success`; still only `tracing::warn!` on create error, no id to push).

4. **`Drop for T2tHandler`**: replace the `if let Some(log_id) = self.log_id` block with one that clones `self.connection_log_ids` (plus `self.pool`) and spawns a task that calls `ConnectionLog::set_ended` for each collected id in sequence, logging a warning per failure (same error handling as today, just looped).

5. **`tarpit::log_hard_ban`** (`crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`): after `ConnectionLog::create` succeeds, immediately call `ConnectionLog::set_ended(&pool, log.id)` — this connection is already closed by the time this function runs (mirrors the pattern `banner_drip.rs::run` uses at the end of its drip loop, just without a delay since there's no drip phase here).

No DB/model changes needed — `ConnectionLog::set_ended` (`crates/tunnel2tunnel-core/src/models/connection_log.rs:81`) already does the right single-row `UPDATE ... SET ended_at = NOW()`.

## Verification

1. `cargo build -p t2t` to confirm it compiles (signature change to `&mut self` on `log_auth_failure`, field rename).
2. Run the stack locally per `CLAUDE.md`'s "Running locally" section.
3. Manually exercise each path and check the `connection_logs` row afterward (via `EntityDetailPage.vue` or `AdminConnectionLogsPage.vue`, or a direct `SELECT id, fail_reason, success_reason, tarpit_action, started_at, ended_at FROM connection_logs ORDER BY started_at DESC LIMIT 10;`):
   - **OK**: register an entity/key, `ssh` in successfully, then disconnect — confirm `ended_at` populates (re-verifying the already-believed-correct path).
   - **Fail**: attempt auth with an unregistered key — confirm the row gets `ended_at` once the client connection closes.
   - **Ban**: trigger a hard ban (repeated failures past threshold, or an admin ban rule) and make one more connection attempt while banned — confirm that row's `ended_at` is set immediately (same request cycle).
   - **Trap**: trigger fake-shell or slow-auth tarpit and disconnect from it — confirm `ended_at` populates. Confirm banner-drip trap still works as before (regression check, not expected to change).
