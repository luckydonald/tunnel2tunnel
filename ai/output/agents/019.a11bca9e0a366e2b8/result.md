Task: research soft-delete mixin for `tarpit_thresholds`/`ban_rules`. Report:

**1. Mixin structs** — `crates/tunnel2tunnel-core/src/timestamps.rs`:
```rust
pub struct Timestamps { created_at: OffsetDateTime, updated_at: OffsetDateTime }
pub struct SoftDelete { deleted_at: Option<OffsetDateTime> }  // #[serde(with="...::option")]
pub struct TimestampsSoftDelete {
    #[sqlx(flatten)] #[serde(flatten)] pub timestamps: Timestamps,
    #[sqlx(flatten)] #[serde(flatten)] pub soft_delete: SoftDelete,
}
```
Already exactly the reusable mixin wanted — no new type needed, just apply `TimestampsSoftDelete` where `Timestamps` is used now.

**2. Working example** — `ssh_key.rs`: struct has `#[sqlx(flatten)] ts: TimestampsSoftDelete`; no DB trigger sets `deleted_at` — plain `soft_delete()` fn does `UPDATE ssh_keys SET deleted_at = NOW() WHERE id=$1 AND deleted_at IS NULL`, returns bool. All list/find queries manually add `AND deleted_at IS NULL` (no partial index/view, just repeated WHERE clause).

**3. `tarpit_threshold.rs`** (full, 96 lines): struct has `ts: Timestamps` (not soft-delete). `list_all`/`list_enabled` plain SELECT *. `delete()` = hard `DELETE FROM tarpit_thresholds WHERE id=$1`.

**4. `ban_rule.rs`** (full, 85 lines): struct has `ts: Timestamps` too (flattened, serde flatten — inconsistent with tarpit_threshold.rs which flattens sqlx only, not serde... check: ban_rule has `#[serde(flatten)]` too, tarpit_threshold's `ts` field lacks `#[serde(flatten)]`, minor inconsistency). `delete()` = hard `DELETE FROM ban_rules WHERE id=$1`.

**5. Migrations**: latest is `012_connection_log_tarpit_reference.sql`. Next number: **013**.

**6/7. Delete call sites**:
- `crates/tunnel2tunnel-web/src/routes/tarpit.rs:176` `delete_ban_rule` handler → `BanRule::delete`
- `crates/tunnel2tunnel-web/src/routes/tarpit.rs:330` `delete_tarpit_threshold` handler → `TarpitThreshold::delete`
- `crates/t2t/tests/tarpit_e2e.rs:379` test cleanup → `TarpitThreshold::delete`
Both web handlers are plain `DELETE /api/admin/ban-rules/{id}` and `DELETE /api/admin/tarpit-thresholds/{id}`, return 204/404 based on bool.

**8. Frontend**: `AdminBanRulesPage.vue` has `handleDelete`/`handleDeleteThreshold` calling `adminApi.deleteBanRule(id)`/`deleteTarpitThreshold(id)` (`admin.ts:197-198`, `222-223`, both plain `DELETE` fetch, `×` button, row removed from list client-side after 204). No soft-delete/restore UI concept exists anywhere in frontend today.

**9. `tarpit/mod.rs` engine**: `refresh_once` (line 415) calls `TarpitThreshold::list_enabled` (line 422, `WHERE enabled` — soft-delete would just add `AND deleted_at IS NULL`, naturally excludes deleted rows) and `BanRule::list_active` (line 423, `WHERE active_until IS NULL OR active_until > NOW()` — needs `AND deleted_at IS NULL` added too). `resolve_outcome`'s existing "rule not found in current list → lenient fallback" path (comment at line 610: "The rule that originally tripped the ban has since been deleted —") **already handles this exact case correctly and needs no changes** — a soft-deleted row simply won't appear in `list_enabled`/`list_active`'s result set, same as a hard-deleted one, and old `connection_logs` FK references stay valid (soft-delete keeps the row, so `ON DELETE SET NULL` never even fires).

Out of scope note: `tarpit_threshold.rs`'s `ts: Timestamps` field is missing `#[serde(flatten)]` (present on `ban_rule.rs` and `ssh_key.rs`) — pre-existing inconsistency, not caused by this research, worth fixing while touching this struct anyway.