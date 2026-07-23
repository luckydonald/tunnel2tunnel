# Tarpit thresholds: single global rule → table of multiple rules

## Context

`AdminBanRulesPage.vue`'s "Global Thresholds" card lets admins configure exactly **one** count+window pair (e.g. "5 fails in 10 min") for auto-triggering a ban, backed by 3 `settings` kv rows (`tarpit_threshold_count`, `tarpit_threshold_window_seconds`, `tarpit_enabled`). User wants **multiple simultaneous windows** — e.g. `5 in 60 min` + `20 in 24h` + `50 in 7 days` + `100 in 30 days` — any one of which independently triggers a ban. This plan is scoped to that refactor only (a follow-up strike-count display in the connection-log tables was discussed but is explicitly deferred/out of scope here).

Current architecture (confirmed via code read):
- `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`: one `Thresholds { count, window, enabled }` (`SharedThresholds = Arc<Mutex<Thresholds>>`), refreshed from the 3 settings keys every 30s (`refresh_once`, lines ~291-317). `TarpitEntry` per identity (`peer_ip` or `user:{uuid}` key) holds one `fail_count`/`window_start` pair; `record_failure()` (lines 130-159) does reset-on-stale-window counting, bans when `fail_count >= thresholds.count`. `trigger_count` only drives round-robin tarpit-method selection, not ban logic.
- Admin UI: `AdminBanRulesPage.vue` combines the ban_rules CRUD table (good template to copy) with the single-row threshold settings form (lines 107-125) backed by `GET/PUT /api/admin/tarpit-settings` (`crates/tunnel2tunnel-web/src/routes/tarpit.rs:177-243`).
- `BanRule` model (`crates/tunnel2tunnel-core/src/models/ban_rule.rs`) is the CRUD template to follow: `list_all`/`create`/`delete`, `#[sqlx(flatten)] ts: Timestamps`, id generated via `Uuid::now_v7()` in Rust despite DB default.
- Next migration number: `009` (`008_tarpit.sql` is latest).

## Migration `migrations/009_tarpit_thresholds.sql`
- `CREATE TABLE tarpit_thresholds (id UUID PRIMARY KEY DEFAULT uuidv7(), fail_count INT NOT NULL CHECK (fail_count > 0), window_seconds BIGINT NOT NULL CHECK (window_seconds > 0), enabled BOOLEAN NOT NULL DEFAULT true, created_at/updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())` + `CREATE TRIGGER timestamps ... set_timestamps()` (copy `008_tarpit.sql:83-84` pattern).
- Backfill one row from the existing `tarpit_threshold_count`/`tarpit_threshold_window_seconds` settings values (or defaults 5/600 if unset), then `DELETE FROM settings WHERE key IN ('tarpit_threshold_count', 'tarpit_threshold_window_seconds')`. Keep `tarpit_enabled` in `settings` as-is — it's the global kill switch, orthogonal to individual rule rows.

## `crates/tunnel2tunnel-core/src/models/tarpit_threshold.rs` (new)
- `TarpitThreshold { id: Uuid, fail_count: i32, window_seconds: i64, enabled: bool, ts: Timestamps }`, same shape/derives as `BanRule`.
- `list_all(pool)`, `list_enabled(pool)` (mirrors `BanRule::list_active`), `create(pool, fail_count, window_seconds, enabled)`, `update(pool, id, fail_count, window_seconds, enabled)`, `delete(pool, id) -> bool`.

## `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`
- Replace `Thresholds`/`SharedThresholds` with `SharedThresholds = Arc<Mutex<Vec<TarpitThreshold>>>` (only enabled rows).
- `TarpitEntry` gets `tallies: HashMap<Uuid, (fail_count: u32, window_start: Instant)>` keyed by threshold-rule id (replacing the single `fail_count`/`window_start` fields).
- `record_failure()` iterates every rule in the shared `Vec`, updates that rule's tally (same reset-on-stale-window logic per rule), and bans (`trigger_count += 1`, `banned = ...`) if **any** rule's tally reaches its `fail_count`. `clear_fail_tally_on_success()` clears all tallies.
- `refresh_once()`: swap the 3-key settings read for `TarpitThreshold::list_enabled(pool)`; keep the separate `tarpit_enabled` global-toggle read as-is (short-circuits `record_auth_failure`/`record_auth_success` same as today).

## `crates/tunnel2tunnel-web/src/routes/tarpit.rs`
- Keep `GET/PUT /api/admin/tarpit-settings` but shrink its body to `{ enabled: bool }` only (the global kill switch).
- Add CRUD: `GET /api/admin/tarpit-thresholds` (list all), `POST /api/admin/tarpit-thresholds` (create, validate `fail_count > 0` / `window_seconds > 0`, same style as `validate_create_ban_rule`), `PUT /api/admin/tarpit-thresholds/{id}` (update), `DELETE /api/admin/tarpit-thresholds/{id}`. Register in `crates/tunnel2tunnel-web/src/lib.rs` alongside the existing ban-rule routes.

## Frontend
- `frontend/src/api/admin.ts`: shrink `TarpitSettings` to `{ enabled: boolean }`; add `TarpitThreshold { id, fail_count, window_seconds, enabled, created_at, updated_at }` + `listTarpitThresholds/createTarpitThreshold/updateTarpitThreshold/deleteTarpitThreshold` methods (same call shape as the existing `listBanRules`/`createBanRule`/`deleteBanRule`).
- `frontend/src/pages/AdminBanRulesPage.vue`: replace the single "Global Thresholds" card with (a) the enabled-toggle-only card, and (b) a new "Threshold rules" table — add-row form (fail_count number input, window input, submit) + `data-table` listing existing rules with an enabled checkbox per row and a delete button — copying the existing Ban Rules add-form/table pattern already on this page (lines ~127-175).

## Verification
- `cargo test -p tunnel2tunnel-core -p tunnel2tunnel-web -p tunnel2tunnel-ssh` (existing tarpit unit/e2e tests, especially `crates/t2t/tests/tarpit_e2e.rs` and `crates/tunnel2tunnel-core/tests/tarpit_models.rs`, must be updated for the `Vec<TarpitThreshold>` shape and continue to pass).
- Run backend+frontend locally (per CLAUDE.md), add 2-3 threshold rules with different windows via the admin page, trigger failed SSH auths from one peer_ip, confirm ban triggers per whichever rule is satisfied first, and that editing/disabling/deleting a rule takes effect (within the 30s refresh cycle).

## Deferred (not in this plan)
- Showing per-row "strike" counts against these thresholds in the connection-log tables (`AdminConnectionLogsPage.vue` / `EntityDetailPage.vue`) — separate follow-up.
