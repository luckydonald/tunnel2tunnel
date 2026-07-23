# Trap vs Ban actions on tarpit threshold rules + decision tracking in connection logs

## Context

Today every `tarpit_thresholds` rule (fail_count + window_seconds) that trips always engages the tarpit (banner_drip/slow_auth/fake_shell round-robin) — there's no way to configure "X is too spammy even for our tarpit, just reject instantly." User wants each rule to carry an action: **Trap** (existing tarpit behavior) or **Ban** (instant reject, no games).

Separately, admin-created `ban_rules` (the "Ban" button in the connection-log UI) are merged into the *same* in-memory ban state as threshold trips (`tarpit/mod.rs` `refresh_once`), and `decide_pre_auth_tarpit`/`decide_in_auth_tarpit` **always** round-robin to a tarpit method for anything banned — meaning an admin clicking "Ban" today actually still tarpits the target rather than instantly rejecting it. This plan makes admin-triggered bans truly instant, which is a behavior change/bugfix, not just new configuration — flagging this since it's implicit in "Admin Ban" being a hard block by definition.

Connection logs will record which of three decisions applied to a rejected/tarpitted attempt — `trap`, `auto_ban` (threshold rule configured as Ban), or `admin_ban` (admin clicked Ban) — plus a reference to *why*: the triggering `tarpit_thresholds` row for `trap`/`auto_ban`, or the admin user id for `admin_ban`.

## Data model changes

**`migrations/011_tarpit_threshold_action.sql`**
```sql
ALTER TABLE tarpit_thresholds ADD COLUMN action TEXT NOT NULL DEFAULT 'trap' CHECK (action IN ('trap', 'ban'));
```
Default `'trap'` preserves current behavior for the existing seeded rule.

**`migrations/012_connection_log_ban_decision.sql`**
```sql
ALTER TABLE connection_logs ADD COLUMN ban_decision TEXT CHECK (ban_decision IS NULL OR ban_decision IN ('trap', 'auto_ban', 'admin_ban'));
ALTER TABLE connection_logs ADD COLUMN tarpit_threshold_id UUID REFERENCES tarpit_thresholds(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD COLUMN banned_by_admin_id UUID REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD CONSTRAINT connection_logs_admin_ban_check
  CHECK (banned_by_admin_id IS NULL OR ban_decision = 'admin_ban');
ALTER TABLE connection_logs ADD CONSTRAINT connection_logs_ban_decision_admin_check
  CHECK (ban_decision != 'admin_ban' OR banned_by_admin_id IS NOT NULL);
```
`tarpit_threshold_id` is set for `trap` (when a rule was actually the cause — see fallback note below) and `auto_ban`; `banned_by_admin_id` only for `admin_ban`.

**`tunnel2tunnel-core::models::tarpit_threshold::TarpitThreshold`**: add `action: String`; thread through `create`/`update` (new param), keep `list_all`/`list_enabled` unaffected (`SELECT *`).

**`tunnel2tunnel-core::models::connection_log::ConnectionLog`**: add `ban_decision: Option<String>`, `tarpit_threshold_id: Option<Uuid>`, `banned_by_admin_id: Option<Uuid>`; add matching params to `create()` (after `tarpit_method`).

## Tarpit engine (`crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`)

New types:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BanSource { Threshold(Uuid), Admin(Uuid) }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TarpitOutcome {
    None,
    Trap { method: TarpitMethod, threshold_id: Option<Uuid> },
    Ban { source: BanSource },
}
```
`TarpitEntry` gains `ban_source: Option<BanSource>`, set wherever `entry.banned` is currently set:
- In `record_failure`: when picking which tripped rule "wins" (multiple rules can trip in one call), prefer any rule with `action == "ban"` over `"trap"`; among same-action ties prefer the longest window (mirrors today's "longest window wins ban duration" logic, which stays unchanged — duration keeps using the max window across *all* tripped rules regardless of action). Store the winning rule's id as `BanSource::Threshold(id)`.
- In `refresh_once`'s `ban_rules` merge: set `entry.ban_source = Some(BanSource::Admin(rule.created_by))` alongside the existing `entry.banned = ...` assignment.

New shared resolver (pure, unit-testable):
```rust
fn resolve_outcome(ban_source: &BanSource, trigger_count: u64, rules: &[TarpitThreshold], allow_banner_drip: bool) -> TarpitOutcome
```
- `BanSource::Admin(id)` → always `Ban { source: Admin(id) }` (admin bans never trap).
- `BanSource::Threshold(id)` → look up the rule by id in `rules`:
  - not found (rule since deleted) → fall back to `Trap { method: round_robin(trigger_count), threshold_id: None }` (lenient default — a vanished rule shouldn't hard-ban forever).
  - found, `action == "ban"` → `Ban { source: Threshold(id) }`.
  - found, `action == "trap"` → `Trap { method: round_robin(trigger_count), threshold_id: Some(id) }` (with the existing banner-drip→slow-auth downgrade only applied by the in-auth caller, same as today).

`decide_pre_auth_tarpit`/`decide_in_auth_tarpit` both gain a `thresholds: &SharedThresholds` param, call `resolve_outcome` after checking `is_banned`, and return `TarpitOutcome` instead of `Option<TarpitMethod>`. Pre-auth keeps its existing `peer_ip_has_known_good_history` banner-drip-ineligibility check, applied only when the resolved outcome is `Trap{method: BannerDrip, ..}` (downgrade to `SlowAuth`, same as today).

## `crates/tunnel2tunnel-ssh/src/lib.rs`

- Accept loop: match the new `TarpitOutcome` instead of `Option<TarpitMethod>`:
  - `Ban{source}` → `tokio::spawn` a small new `tarpit::log_hard_ban(pool, peer_ip, source)` (writes one `ConnectionLog::create` row: `fail_reason: "ip banned"`, `ban_decision` derived from `source`, `tarpit_threshold_id`/`banned_by_admin_id` set accordingly, everything else `None`), then `continue` without ever touching `russh` — the socket drops when the loop iteration ends, closing the connection instantly.
  - `Trap{method: BannerDrip, threshold_id}` → existing `banner_drip::run` spawn, now also passed `threshold_id` so it logs `tarpit_threshold_id`/`ban_decision: "trap"` instead of the old hardcoded values.
  - `Trap{method, threshold_id}` (other methods) / `None` → existing handler path, caching the whole `TarpitOutcome` on the handler (replacing today's `handler.tarpit_method = pre_auth_tarpit`).
- `T2tHandler`: rename `tarpit_method: Option<TarpitMethod>` → `tarpit_outcome: Option<TarpitOutcome>`; `resolve_tarpit_method` → `resolve_tarpit_outcome(user_id) -> TarpitOutcome` (lazily calls `decide_in_auth_tarpit`, caches).
- Each of the ~7 auth-failure call sites (`auth_password`, `auth_keyboard_interactive`, and the 5 failure branches inside `auth_publickey`) currently do `let method = self.resolve_tarpit_method(...).await; ...; if method == Some(FakeShell) {...} if method == Some(SlowAuth) {...}`. Mechanically becomes: `let outcome = self.resolve_tarpit_outcome(...).await;` then a `match` on `TarpitOutcome::Trap{method: FakeShell,..}` / `Trap{method: SlowAuth,..}` / `_ => {}` (covers `None`, `Ban`, and the pre-auth-only `BannerDrip` case which can't occur here) — same fallthrough-to-`Auth::Reject` behavior as today for `None`/`Ban`, since a hard ban and a plain reject look identical on the wire from inside a `Handler` callback (russh gives no third "just drop" return from here).
- `log_auth_failure`: add an `outcome: &TarpitOutcome` param; derives `tarpit_method` (only set for `Trap`), `ban_decision` (`"trap"`/`"auto_ban"`/`"admin_ban"`/`None`), `tarpit_threshold_id`, `banned_by_admin_id` for the `ConnectionLog::create` call. `log_auth_success` is untouched — a successful login never reaches this path while banned (existing behavior, unaffected).

## Web layer (`crates/tunnel2tunnel-web/src/routes/tarpit.rs`, `entities.rs`)

- `TarpitThresholdBody`/`TarpitThresholdResponse`: add `action: String`; `validate_threshold_body` also checks `action` is `"trap"` or `"ban"` (same style as `validate_create_ban_rule`'s scope_type check).
- `create_tarpit_threshold`/`update_tarpit_threshold`: pass `action` through to the model.
- `ConnLogResponse` (`entities.rs`): add `ban_decision: Option<String>`, `tarpit_threshold_id: Option<Uuid>`, `banned_by_admin_id: Option<Uuid>`.
- No change needed to `create_ban_rule`/`CreateBanRuleBody` — `ban_rules.created_by` already captures the admin, and the tarpit engine now reads it during `refresh_once`.

## Frontend

- `frontend/src/api/admin.ts`: `TarpitThreshold`/`TarpitThresholdParams` gain `action: 'trap' | 'ban'`; `ConnLog` gains `ban_decision`, `tarpit_threshold_id`, `banned_by_admin_id`.
- `frontend/src/labels.ts`: add `tarpitActionLabel`/`tarpitActionOptions` (Trap/Ban) and `banDecisionLabel` (Trap/Auto-ban/Admin ban), following the existing `banScopeTypeLabel`/`Options` pattern.
- `AdminBanRulesPage.vue`: add an "Action" column (select: Trap/Ban) to the Threshold Rules add-form and table; add a `:id="`threshold-${t.id}`"` anchor per row (same pattern as the `AdminUsersPage.vue` `#user-{id}` anchors added previously) so connection-log rows can deep-link to the specific rule.
- `AdminConnectionLogsPage.vue` (and `EntityDetailPage.vue`'s log table): add a "Decision" column showing `banDecisionLabel[l.ban_decision]`; when `tarpit_threshold_id` is set, link to `/admin/ban-rules#threshold-{id}`; when `banned_by_admin_id` is set, link to `/admin/users#user-{id}` (reusing the `RouterLink` pattern already added for `user_id` in the previous session).

## Verification

- `cargo test -p tunnel2tunnel-core -p tunnel2tunnel-ssh -p tunnel2tunnel-web` — update the `tarpit/mod.rs` unit-test `rule()` helper to include `action: "trap".to_string()`; add new unit tests: a `"ban"`-action rule trips straight to `Ban`, a mixed trap+ban simultaneous trip prefers `Ban`, and `resolve_outcome` falls back to `Trap` when the referenced rule id is missing.
- Run migrations 011/012 against the local Postgres (`podman start t2t-pg` if needed) and re-run the existing `tarpit_models`/`tarpit_e2e` integration suites — must stay green since `action` defaults to `'trap'`.
- Add one new `t2t` e2e test asserting a threshold rule configured with `action = 'ban'` causes an immediate `Auth::Reject` with no slow-auth delay and no fake-shell `Auth::Accept`.
- Manually: in the admin UI, add a Ban-action threshold rule, drive failed attempts past it, confirm instant rejection (no delay) and a `connection_logs` row with `ban_decision = 'auto_ban'` + the rule id; click "Ban" on a log row, confirm the *next* attempt from that identity is instantly rejected (not tarpitted) with `ban_decision = 'admin_ban'` + the admin's id.
