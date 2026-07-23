# Multi-window tarpit thresholds + per-row strike counts in connection logs

## Context

Started as "add a strike-number column to the connection log view." Turns out the admin-configurable tarpit ban trigger (`AdminBanRulesPage.vue` "Global Thresholds" card, backed by a single `settings` kv row: `tarpit_threshold_count` / `tarpit_threshold_window_seconds` / `tarpit_enabled`) only supports **one** count+window pair today (e.g. "5 fails in 10 min"). User wants **multiple simultaneous windows** configurable as rows in a table — e.g. `5 in 60 min` + `20 in 24h` + `50 in 7 days` + `100 in 30 days` — any one of which can independently trigger a ban. The connection-log "strike" column should then show, per row, how that attempt counts against each configured window — which is what "which strike it is" actually means once thresholds aren't a single global number.

Current architecture (confirmed via code read):
- `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`: one `Thresholds { count, window, enabled }` (`SharedThresholds = Arc<Mutex<Thresholds>>`), refreshed from the 3 settings keys every 30s (`refresh_once`, lines ~291-317). `TarpitEntry` per identity (`peer_ip` or `user:{uuid}` key) holds one `fail_count`/`window_start` pair; `record_failure()` (lines 130-159) does reset-on-stale-window counting, bans when `fail_count >= thresholds.count`. `trigger_count` only drives round-robin tarpit-method selection, not ban logic.
- Ban decision is **purely in-memory** — `ConnectionLog::count_recent_failures_for_peer_ip/_for_user` exist but are dead code in production (only called from tests).
- Admin UI: `AdminBanRulesPage.vue` combines the ban_rules CRUD table (good template to copy) with the single-row threshold settings form (lines 107-125) backed by `GET/PUT /api/admin/tarpit-settings` (`crates/tunnel2tunnel-web/src/routes/tarpit.rs:177-243`).
- `BanRule` model (`crates/tunnel2tunnel-core/src/models/ban_rule.rs`) is the CRUD template to follow: `list_all`/`create`/`delete`, `#[sqlx(flatten)] ts: Timestamps`, id generated via `Uuid::now_v7()` in Rust despite DB default.
- Next migration number: `009` (`008_tarpit.sql` is latest).

## Part A — Tarpit thresholds become a table of rules

### Migration `migrations/009_tarpit_thresholds.sql`
- `CREATE TABLE tarpit_thresholds (id UUID PRIMARY KEY DEFAULT uuidv7(), fail_count INT NOT NULL CHECK (fail_count > 0), window_seconds BIGINT NOT NULL CHECK (window_seconds > 0), enabled BOOLEAN NOT NULL DEFAULT true, created_at/updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())` + `CREATE TRIGGER timestamps ... set_timestamps()` (copy `008_tarpit.sql:83-84` pattern).
- Backfill one row from the existing `tarpit_threshold_count`/`tarpit_threshold_window_seconds` settings values (or defaults 5/600 if unset), then `DELETE FROM settings WHERE key IN ('tarpit_threshold_count', 'tarpit_threshold_window_seconds')`. Keep `tarpit_enabled` in `settings` as-is — it's the global kill switch, orthogonal to individual rule rows.

### `crates/tunnel2tunnel-core/src/models/tarpit_threshold.rs` (new)
- `TarpitThreshold { id: Uuid, fail_count: i32, window_seconds: i64, enabled: bool, ts: Timestamps }`, same shape/derives as `BanRule`.
- `list_all(pool)`, `list_enabled(pool)` (mirrors `BanRule::list_active`), `create(pool, fail_count, window_seconds, enabled)`, `update(pool, id, fail_count, window_seconds, enabled)`, `delete(pool, id) -> bool`.

### `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`
- Replace `Thresholds`/`SharedThresholds` with `SharedThresholds = Arc<Mutex<Vec<TarpitThreshold>>>` (only enabled rows).
- `TarpitEntry` gets `tallies: HashMap<Uuid, (fail_count: u32, window_start: Instant)>` keyed by threshold-rule id (replacing the single `fail_count`/`window_start` fields).
- `record_failure()` iterates every rule in the shared `Vec`, updates that rule's tally (same reset-on-stale-window logic per rule), and bans (`trigger_count += 1`, `banned = ...`) if **any** rule's tally reaches its `fail_count`. `clear_fail_tally_on_success()` clears all tallies.
- `refresh_once()`: swap the 3-key settings read for `TarpitThreshold::list_enabled(pool)`; keep the separate `tarpit_enabled` global-toggle read as-is (short-circuits `record_auth_failure`/`record_auth_success` same as today).

### `crates/tunnel2tunnel-web/src/routes/tarpit.rs`
- Keep `GET/PUT /api/admin/tarpit-settings` but shrink its body to `{ enabled: bool }` only (the global kill switch).
- Add CRUD: `GET /api/admin/tarpit-thresholds` (list all), `POST /api/admin/tarpit-thresholds` (create, validate `fail_count > 0` / `window_seconds > 0`, same style as `validate_create_ban_rule`), `PUT /api/admin/tarpit-thresholds/{id}` (update), `DELETE /api/admin/tarpit-thresholds/{id}`. Register in `crates/tunnel2tunnel-web/src/lib.rs` alongside the existing ban-rule routes.

### Frontend
- `frontend/src/api/admin.ts`: shrink `TarpitSettings` to `{ enabled: boolean }`; add `TarpitThreshold { id, fail_count, window_seconds, enabled, created_at, updated_at }` + `listTarpitThresholds/createTarpitThreshold/updateTarpitThreshold/deleteTarpitThreshold` methods (same call shape as the existing `listBanRules`/`createBanRule`/`deleteBanRule`).
- `frontend/src/pages/AdminBanRulesPage.vue`: replace the single "Global Thresholds" card with (a) the enabled-toggle-only card, and (b) a new "Threshold rules" table — add-row form (fail_count number input, window input, submit) + `data-table` listing existing rules with an enabled checkbox per row and a delete button — copying the existing Ban Rules add-form/table pattern already on this page (lines ~127-175).

## Part B — Per-row "strikes" on connection log tables

For each **enabled** `tarpit_threshold` rule, show a rolling count: how many failed attempts from that row's `peer_ip` fall within `[started_at - window_seconds, started_at]`, inclusive of the row itself (mirrors the actual ban-trigger check, just computed retroactively/read-only instead of from in-memory state). This directly answers "which strike is this" per configured window, and stays correct even as rules are added/edited/removed later (recomputed live, not persisted).

### `crates/tunnel2tunnel-core/src/models/connection_log.rs`
- `search()` and `list_for_entity()` first fetch the current `TarpitThreshold::list_enabled(pool)` rows, then build a query with one extra window-function column per rule:
  ```sql
  SUM(CASE WHEN fail_reason IS NOT NULL THEN 1 ELSE 0 END) OVER (
    PARTITION BY peer_ip ORDER BY started_at
    RANGE BETWEEN interval '<window_seconds> seconds' PRECEDING AND CURRENT ROW
  ) AS strike_<n>
  ```
  bound via `$N` parameter per rule (dynamic SQL string built in Rust, same way `search()` already conditionally builds its WHERE clause) — `NULL` result (i.e. value only meaningful) only shown for rows where `fail_reason IS NOT NULL`, since successes aren't "strikes."
- Return shape becomes `(rows: Vec<ConnectionLogWithStrikes>, total, thresholds_used: Vec<TarpitThreshold>)` so the web layer knows which threshold each strike column corresponds to (id/fail_count/window_seconds) — needed for response labeling and for the frontend to render column headers like "3/5 (10m)".
- Add a small struct `RowStrikes { threshold_id: Uuid, count: i64 }` — one `Vec<RowStrikes>` per `ConnectionLog` row.

### `crates/tunnel2tunnel-web/src/routes/entities.rs` / `tarpit.rs`
- `ConnLogResponse` gains `strikes: Vec<{ threshold_id: Uuid, fail_count: i32, window_seconds: i64, count: i64 }>` (denormalized — includes the rule's own configured `fail_count`/`window_seconds` per entry so the frontend can render "count/fail_count in window" without a second lookup).
- `list_connection_logs` and `search_connection_logs` both thread the fetched thresholds through to `ConnLogResponse::from`.

### Frontend
- `frontend/src/api/admin.ts`: `ConnLog.strikes: Array<{ threshold_id: string; fail_count: number; window_seconds: number; count: number }>`.
- `frontend/src/labels.ts`: small `formatWindow(seconds)` helper (e.g. "10m", "24h", "7d") for compact display — reused by both the log tables and the new threshold-rules table in `AdminBanRulesPage.vue`.
- `AdminConnectionLogsPage.vue` and `EntityDetailPage.vue`: add one "Strikes" `<td>` rendering all entries compactly, e.g. `3/5 (10m), 12/20 (24h)`, `—` if no enabled thresholds or row is a success.

## Verification
- `cargo test -p tunnel2tunnel-core -p tunnel2tunnel-web -p tunnel2tunnel-ssh` (existing tarpit unit/e2e tests, especially `crates/t2t/tests/tarpit_e2e.rs` and `crates/tunnel2tunnel-core/tests/tarpit_models.rs`, must be updated for the `Vec<TarpitThreshold>` shape and continue to pass).
- Run backend+frontend locally (per CLAUDE.md), add 2-3 threshold rules with different windows via the admin page, trigger failed SSH auths from one peer_ip, confirm: (1) ban triggers per the smallest-satisfied rule, (2) strike counts in both log tables increment correctly per rule and reset appropriately as attempts age out of each window.

## Open call I made without re-asking (flag if wrong)
- Kept a separate global `tarpit_enabled` boolean kill switch outside the per-rule table (each rule also has its own `enabled` flag) rather than folding "enabled" into being "at least one rule exists."
