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

## Automated tests

Both existing e2e suites already spin up a real `t2t` SSH server against a real Postgres and drive real auth attempts (`crates/t2t/tests/tarpit_e2e.rs` via an in-process `russh::client`, `crates/t2t/tests/tunnel_e2e.rs` via real `ssh` subprocesses) — reuse their existing helpers (`spawn_server`, `connect_client`, `generate_keypair`, `TEST_GUARD`) rather than inventing a new harness. All new assertions read the row back with `sqlx::query_as::<_, ConnectionLog>(...)` directly (every `ConnectionLog` field is `pub`, and `sqlx` is already a regular dependency of the `t2t` crate) since most of these rows have no `entity_id` to key off via `list_for_entity`.

All additions land in `crates/t2t/tests/tarpit_e2e.rs`:

1. **OK** — extend `legit_login_sequence_is_never_tarpitted`: the connection already closes by the time the test reads the row back (the client `Handle` is local to `legit_login_sequence` and drops when that function returns), so just add `assert!(log.ended_at.is_some(), ...)` next to the existing `log.success`/`success_reason` assertions.

2. **Fail** — new test `single_failed_login_marks_ended_at_on_disconnect`: one unregistered-key `authenticate_publickey` attempt in an inner scope (so the client `Handle` drops before the row is queried), then query the latest `connection_logs` row for `peer_ip = '127.0.0.1'` filtered by `started_at >= <test-start time>` (mirrors `auth_none_probe_never_counts_toward_ban`'s `since` pattern), assert `!log.success` and `log.ended_at.is_some()`.

3. **Ban** — extend `ban_action_threshold_rejects_instantly_without_tarpit`: after the existing "closes instantly" assertions, sleep briefly (`log_hard_ban` is `tokio::spawn`ed from the accept loop, not awaited inline) then query the latest row with `tarpit_action = 'ban'` and assert `ended_at.is_some()` — this is the case with no `T2tHandler`/`Drop` at all, so it's the one that most directly exercises the new immediate `set_ended` call in `log_hard_ban`.

4. **Trap** — extend `repeated_bans_eventually_engage_fake_shell`: after reading the bogus `$` prompt, explicitly `drop(handle)` to close the connection, sleep briefly, then query the latest row with `tarpit_action = 'trap' AND tarpit_method = 'fake_shell'` and assert `ended_at.is_some()`.
   - Also extend `repeated_bad_key_attempts_trigger_slow_auth_delay`: after the loop (each iteration's `handle` is already scoped to drop per-iteration), query the latest `peer_ip = '127.0.0.1'` row and assert `ended_at.is_some()` — covers the `slow_auth` trap variant.
   - Banner-drip (`repeated_bans_eventually_engage_banner_drip`) is **not** touched: it already calls `set_ended` correctly today (see Context), and forcing its drip loop to exit for a test would require waiting out `DRIP_INTERVAL` (10s), making the test slow for no regression coverage gained.

## Manual verification

1. `cargo build -p t2t` to confirm it compiles (signature change to `&mut self` on `log_auth_failure`, field rename).
2. `cargo test -p t2t` (requires local Postgres per `CLAUDE.md`) — all four new/extended assertions above should pass.
3. Spot-check via `EntityDetailPage.vue` / `AdminConnectionLogsPage.vue` after a real login+disconnect, matching the automated OK-path coverage.
