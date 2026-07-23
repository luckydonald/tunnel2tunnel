# Soft-delete for time window rules (`tarpit_thresholds` + `ban_rules`)

## Context

`tarpit_thresholds` and `ban_rules` rows are hard-deleted today (`DELETE FROM ...`). Since the Trap/Ban work, `connection_logs` rows can reference either table (`tarpit_threshold_id`, `banned_by_ban_rule_id`, both `ON DELETE SET NULL`). Deleting a rule today silently blanks that reference on every past log row that pointed to it — losing the "why was this banned" context permanently. `resolve_outcome` already treats a missing-row lookup as a lenient fallback, which was designed for exactly this case, but it means live decisions also degrade the moment a rule is deleted, not just historical logs.

Switching both tables to soft-delete fixes this: past `connection_logs` rows keep a live, resolvable reference to the (now-inactive) rule that caused them, while the tarpit engine still excludes soft-deleted rows from active decision-making (same as today's hard delete, since the engine's queries already filter to enabled/active rows and will just add `deleted_at IS NULL`).

The codebase already has exactly this pattern as a reusable mixin — `TimestampsSoftDelete` (`crates/tunnel2tunnel-core/src/timestamps.rs`), used by `User`/`Entity`/`SshKey` — so this is "apply the existing mixin to two more models," not a new abstraction.

## Data model changes

**`migrations/013_tarpit_ban_soft_delete.sql`**
```sql
ALTER TABLE tarpit_thresholds ADD COLUMN deleted_at TIMESTAMPTZ;
ALTER TABLE ban_rules ADD COLUMN deleted_at TIMESTAMPTZ;
```
No backfill needed — new column defaults to `NULL` (not deleted) for all existing rows.

**`tunnel2tunnel-core::models::tarpit_threshold::TarpitThreshold`** and **`ban_rule::BanRule`**:
- Change `ts: Timestamps` → `ts: TimestampsSoftDelete` (matches `ssh_key.rs`'s pattern exactly: `#[sqlx(flatten)] #[serde(flatten)] pub ts: TimestampsSoftDelete`). While touching `tarpit_threshold.rs`, also add the missing `#[serde(flatten)]` on `ts` (present on `ban_rule.rs`/`ssh_key.rs` already, absent here — pre-existing inconsistency noted during research).
- Rename `delete()` → `soft_delete()` on both, following `ssh_key.rs`'s convention: `UPDATE <table> SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL`, returns `bool` (true if a row was actually soft-deleted, false if already gone/missing — same semantics as today's hard-delete return value).
- Add `restore(pool, id) -> bool`: `UPDATE <table> SET deleted_at = NULL WHERE id = $1 AND deleted_at IS NOT NULL`. New capability, made cheap now that delete is non-destructive.
- `list_enabled` (`TarpitThreshold`) / `list_active` (`BanRule`) — the queries the tarpit engine actually runs — gain `AND deleted_at IS NULL`. These are the only queries `tarpit/mod.rs::refresh_once` calls, so no further engine changes needed; `resolve_outcome`'s existing "not found in current list → lenient fallback" path already does the right thing for a soft-deleted row without modification.
- `list_all` (used by the admin UI) stays returning every row **including soft-deleted ones** — the admin page needs to show deleted rules so old connection-log links resolve to something instead of a blank/missing row. Response type gains `deleted_at: Option<OffsetDateTime>` so the frontend can badge/gray it out.

## Web layer (`crates/tunnel2tunnel-web/src/routes/tarpit.rs`)

- `delete_ban_rule` / `delete_tarpit_threshold` handlers: call `soft_delete` instead of `delete` (same route/verb, same 204/404 semantics — no API shape change here).
- New `POST /api/admin/ban-rules/{id}/restore` and `POST /api/admin/tarpit-thresholds/{id}/restore` handlers calling the new `restore()` — 204 on success, 404 if not found/not deleted.
- `BanRuleResponse`/`TarpitThresholdResponse` gain `deleted_at: Option<String>` (rfc3339), threaded from the model.

## Frontend

- `frontend/src/api/admin.ts`: `TarpitThreshold`/`BanRule` types gain `deleted_at: string | null`; add `restoreBanRule(id)`/`restoreTarpitThreshold(id)` calling the new endpoints.
- `AdminBanRulesPage.vue`: rows with `deleted_at` set render with a "Deleted" badge (reuse the existing badge CSS pattern from the Status column work) in place of the delete (`×`) button, plus a "Restore" button calling the new restore API and refetching the list. Non-deleted rows keep today's delete button/behavior unchanged. Existing `:id="`threshold-${t.id}`"`/`:id="`rule-${r.id}`"` anchors stay as-is — they already make deleted rows linkable from `AdminConnectionLogsPage.vue`, which is the point of this change.
- No change needed in `AdminConnectionLogsPage.vue` — its existing links to `#threshold-{id}`/`#rule-{id}` already work correctly once the referenced row still exists (soft-deleted or not).

## Verification

- `cargo test -p tunnel2tunnel-core -p tunnel2tunnel-web` — update any test call sites using `TarpitThreshold::delete`/`BanRule::delete` (e.g. `crates/t2t/tests/tarpit_e2e.rs:379`) to `soft_delete`; add model tests for `soft_delete` (row excluded from `list_enabled`/`list_active`, still present in `list_all`) and `restore` (round-trip).
- Run migration 013 locally; confirm existing rows have `deleted_at = NULL` and are unaffected.
- Manually: delete a threshold/ban rule from the admin UI, confirm it disappears from active enforcement (tarpit engine no longer applies it) but still shows (badged "Deleted") in the admin list with a working Restore button, and that any connection-log row referencing it still resolves the link instead of going blank.
