Here's the full picture of the codebase's data model. Bottom line: **no "strike number" concept exists anywhere yet** — there is no persisted per-attempt sequence number, no `strike_count`/`fail_count` column on `connection_logs`, and no field named "strike" in Rust or TypeScript. It would need to be computed, not just displayed.

## 1. `connection_logs` schema

- `/home/user/git/luckydonald/tunnel2tunnel/migrations/004_connection_logs.sql` (lines 1-21): original table — `id, entity_id, peer_ip, key_fingerprint, login_succeeded, failure_reason, ssh_flags, ports_requested, started_at, ended_at, created_at, updated_at`. No strike/ban column.
- `/home/user/git/luckydonald/tunnel2tunnel/migrations/008_tarpit.sql`: the tarpit/ban migration. Splits `failure_reason` into `fail_reason`/`success_reason`, adds generated `success` column, adds `attempted_password`, `user_id`, and `tarpit_method TEXT CHECK (... IN ('banner_drip','slow_auth','fake_shell'))` (lines 55-56). Also creates `ban_rules` table (lines 67-88) and seeds `settings` with `tarpit_threshold_count` (5), `tarpit_threshold_window_seconds` (600), `tarpit_enabled` (lines 94-98). No column that stores a strike/attempt number per row.

No other migration mentions strike/tarpit/ban.

## 2. Rust model — `ConnectionLog`

`/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/connection_log.rs`

- Struct definition lines 11-29 — fields: `id, entity_id, user_id, peer_ip, key_fingerprint, attempted_password, fail_reason, success_reason, success, tarpit_method, ssh_flags, ports_requested, started_at, ended_at, ts (Timestamps)`. **No strike/attempt-count field.**
- `count_recent_failures_for_peer_ip` (lines 97-111) and `count_recent_failures_for_user` (lines 113-127) compute a live count of failed rows in a time window — this is the closest existing concept to a "strike count," but it's a query-time aggregate, not stored per-row, and it's used only by the tarpit decision engine (not exposed via API).
- `search()` (lines 150-208) is the paginated admin query; it doesn't compute or return any strike ordinal per row either.

## 3. Tarpit/ban-rules feature (in-memory "strikes")

`/home/user/git/luckydonald/tunnel2tunnel-ssh/src/tarpit/mod.rs`:

- `TarpitEntry` struct (lines 69-80): `fail_count: u32` (resets on window/ban), `window_start: Instant`, `trigger_count: u64` (**this is the real "how many times has this identity been banned" counter** — drives round-robin tarpit method selection via `TarpitMethod::round_robin(trigger_count)`), `banner_drip_eligible: bool`, `banned: BanUntil`.
- This state is **purely in-memory** (`TarpitState = Arc<Mutex<HashMap<String, TarpitEntry>>>`, line 99) — never persisted to `connection_logs` or any table. It's reset on server restart and not queryable via the admin API at all.
- `record_failure()` (lines 134-159) increments `fail_count`; when it hits the threshold it bumps `trigger_count` and sets a ban.
- Nothing here writes a per-connection strike/sequence number back into the `connection_logs` row when a `ConnectionLog::create` call happens (see `crates/tunnel2tunnel-ssh/src/lib.rs:261` area where `ConnectionLog::create` is invoked with `tarpit_method` but not a strike count).

## 4. Web API route serving connection logs

`/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/tarpit.rs`:

- `search_connection_logs` handler (lines 47-71), query struct `LogSearchQuery` (lines 22-31: `page, page_size, peer_ip, user_id, success, method, q`), response struct `LogSearchResponse` (lines 33-39: `items: Vec<ConnLogResponse>, total, page, page_size`).
- `ConnLogResponse` itself lives in `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/entities.rs:569-599` — mirrors the DB model 1:1, timestamps formatted as RFC3339 strings. No strike field.
- Route registered in `crates/tunnel2tunnel-web/src/lib.rs:110-111` as `GET /api/admin/connection-logs`.

## 5. Frontend admin page + types

- `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/api/admin.ts`: `ConnLog` interface (lines 14-26) — exact mirror of `ConnLogResponse`, no strike field. `LogSearchParams`/`LogSearchResult` (lines 28-43). `searchConnectionLogs` (lines 158-165) calls `/api/admin/connection-logs`.
- `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/AdminConnectionLogsPage.vue`: table columns defined at lines 120-133 (`<thead>`): Started, Ended, Peer IP, User, Fingerprint, Password, Result, Reason, Tarpit, (actions). Row rendering at lines 136-153. There's also a similar table in `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/EntityDetailPage.vue` (columns around line 668, row rendering ~682) that shares the same `ConnLog` shape and would need the same column added if you want consistency.
- `frontend/src/labels.ts` has `tarpitMethodLabel`/`tarpitMethodOptions` (lines 37-44) as the pattern to follow for any new enum-like label mapping.

## What "strike number" would require

Since nothing currently persists a strike/attempt ordinal per connection-log row, to add this column you'd need to either:
1. **Compute at query time** (cheapest): extend `ConnectionLog::search` (and maybe `list_for_entity`) with a window function like `COUNT(*) OVER (PARTITION BY peer_ip ORDER BY started_at)` filtered to `fail_reason IS NOT NULL` rows, expose it as a new field on `ConnLogResponse`/`ConnLog` (TS), and add a `<th>Strike #</th>` column to both Vue pages.
2. **Persist it** (more invasive): add a new migration (e.g. `009_strike_number.sql`) adding a `strike_number` column to `connection_logs`, populate it at write time in `ConnectionLog::create` — likely by threading the `TarpitEntry.fail_count`/`trigger_count` value from `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs` through `lib.rs` into the `create()` call, similar to how `tarpit_method` is threaded today (see `crates/tunnel2tunnel-ssh/src/lib.rs` lines 235-271 for where `tarpit_method` is resolved before the log write).

Option 2 is more useful since `TarpitEntry.fail_count`/`trigger_count` is the actual "strike" semantics already implemented in-memory for the ban logic — it just isn't currently surfaced or persisted anywhere.