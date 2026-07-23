# Trap vs Ban actions (threshold rules + admin Ban button) + decision tracking in connection logs

## Context

Today every `tarpit_thresholds` rule (fail_count + window_seconds) that trips always engages the tarpit (banner_drip/slow_auth/fake_shell round-robin) — no way to say "too spammy even for the tarpit, just reject instantly." Separately, the admin "Ban" button (`AdminConnectionLogsPage.vue`) creates a `ban_rules` row that today *also* only ever feeds into the same tarpit round-robin (`tarpit/mod.rs` `refresh_once` merges `ban_rules` into the same `TarpitState` map threshold-trips use) — so clicking "Ban" doesn't actually hard-block anyone today, it just makes them tarpitted too. That's an existing bug this plan fixes along the way.

Design, per this session's discussion: give **both** `tarpit_thresholds` rows and `ban_rules` rows an `action` (`trap` | `ban`). The admin "Ban" button becomes a two-part attached button — `( Trap | Ban )`, a new reusable `MultiButton` component — so a human ban can be either kind too, exactly like a rule. This unifies the two "who caused this ban" sources: an automatic decision is identified by its reference to a `tarpit_thresholds` row (whose `action` says what happened), an admin decision by its reference to a `ban_rules` row (whose `action`, chosen via the button, says what happened) — no separate categorical "trap/auto_ban/admin_ban" enum needed on `connection_logs`, just the action actually taken plus whichever one reference is set.

## Data model changes

**`migrations/011_tarpit_ban_actions.sql`**
```sql
ALTER TABLE tarpit_thresholds ADD COLUMN action TEXT NOT NULL DEFAULT 'trap' CHECK (action IN ('trap', 'ban'));
ALTER TABLE ban_rules ADD COLUMN action TEXT NOT NULL DEFAULT 'ban' CHECK (action IN ('trap', 'ban'));
```
Defaults preserve current behavior (threshold rules already always trapped; existing ban_rules default to `'ban'`, matching the button's original name/intent — this is the fix, not a behavior surprise for new rows).

**`migrations/012_connection_log_tarpit_reference.sql`**
```sql
ALTER TABLE connection_logs ADD COLUMN tarpit_action TEXT CHECK (tarpit_action IS NULL OR tarpit_action IN ('trap', 'ban'));
ALTER TABLE connection_logs ADD COLUMN tarpit_threshold_id UUID REFERENCES tarpit_thresholds(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD COLUMN banned_by_ban_rule_id UUID REFERENCES ban_rules(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD CONSTRAINT connection_logs_ban_reference_xor_check
  CHECK (tarpit_threshold_id IS NULL OR banned_by_ban_rule_id IS NULL);
```
`tarpit_action` records what actually happened (`'trap'` also covers the existing `tarpit_method` banner_drip/slow_auth/fake_shell flavor, unchanged). The two reference columns stay strictly exclusive — a given ban event comes from exactly one source, a threshold trip *or* an admin rule match, never both — and now **both** get set whenever their source decided the outcome, regardless of whether that source's configured action was `trap` or `ban` (confirmed with user: reference tracks *source*, not *action*). The admin behind an admin-triggered event is reached via `banned_by_ban_rule_id → ban_rules.created_by`, no direct user FK needed on `connection_logs`.

**`tunnel2tunnel-core::models::tarpit_threshold::TarpitThreshold`**: add `action: String`; thread through `create`/`update`.
**`tunnel2tunnel-core::models::ban_rule::BanRule`**: add `action: String`; thread through `create` (no `update` exists today — fine, rules are delete+recreate).
**`tunnel2tunnel-core::models::connection_log::ConnectionLog`**: add `tarpit_action: Option<String>`, `tarpit_threshold_id: Option<Uuid>`, `banned_by_ban_rule_id: Option<Uuid>`; matching new params on `create()`.

## Tarpit engine (`crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`)

New types:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BanSource { Threshold(Uuid), AdminRule(Uuid) } // Uuid = tarpit_thresholds.id / ban_rules.id

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TarpitOutcome {
    None,
    Trap { method: TarpitMethod, source: Option<BanSource> },
    Ban { source: BanSource },
}
```
`Trap` and `Ban` both carry the same `BanSource` shape now (only `None` when the referenced row was deleted after the ban took effect — see fallback below) — kept as separate enum variants because control flow genuinely differs (`Ban` short-circuits straight to reject; `Trap` still goes through round-robin/slow-auth/fake-shell). `ThresholdConfig` gains a second list: `pub ban_rules: Vec<BanRule>` alongside `pub rules: Vec<TarpitThreshold>` (both refreshed together in `refresh_once`, replacing the current one-off `BanRule::list_active` merge-and-discard). `TarpitEntry` gains `ban_source: Option<BanSource>`, set:
- In `record_failure`: when multiple rules trip in one call, prefer any `action == "ban"` rule over `"trap"`; tie-break by longest window (ban *duration* keeps using the max window across all tripped rules regardless of action, unchanged). Store the winning rule's id as `BanSource::Threshold(id)`.
- In `refresh_once`'s ban_rules merge: `entry.ban_source = Some(BanSource::AdminRule(rule.id))` (not the admin's user id directly — reached via the rule, matching the Threshold pattern and staying live-editable if the rule is later edited... though today `ban_rules` has no `update`, only delete+recreate, so this mainly matters for consistency).

Shared resolver (pure, unit-testable):
```rust
fn resolve_outcome(ban_source: &BanSource, trigger_count: u64, rules: &[TarpitThreshold], ban_rules: &[BanRule]) -> TarpitOutcome
```
- `Threshold(id)`: look up in `rules`; not found (rule deleted after the ban took effect) → lenient fallback `Trap{method: round_robin(trigger_count), source: None}` (no reference — the row it would've pointed to is gone); found+`"ban"` → `Ban{source: Threshold(id)}`; found+`"trap"` → `Trap{method: round_robin(trigger_count), source: Some(Threshold(id))}`.
- `AdminRule(id)`: same shape, looked up in `ban_rules` instead; not found → same lenient `Trap{..., source: None}` fallback; found+`"ban"` → `Ban{source: AdminRule(id)}`; found+`"trap"` → `Trap{method: round_robin(trigger_count), source: Some(AdminRule(id))}`.

`decide_pre_auth_tarpit`/`decide_in_auth_tarpit` gain a `thresholds: &SharedThresholds` param (already exists as a field, just needs passing to these two call sites) and return `TarpitOutcome` instead of `Option<TarpitMethod>`. Pre-auth keeps the banner-drip-ineligibility downgrade (only applies when resolved outcome is `Trap{method: BannerDrip, ..}`, preserving `source` as-is).

## `crates/tunnel2tunnel-ssh/src/lib.rs`

- Accept loop matches `TarpitOutcome`: `Ban{source}` → spawn a new `tarpit::log_hard_ban(pool, peer_ip, source)` (one `ConnectionLog::create` row: `fail_reason: "ip banned"`, `tarpit_action: "ban"`, the reference column matching `source` set) then `continue` — socket drops, connection closes instantly, `russh` never touches it. `Trap{method: BannerDrip, source}` → existing `banner_drip::run` spawn, now passed `source` to log the reference correctly. `Trap{other, ..}` / `None` → existing handler path, caching the whole outcome (replacing `handler.tarpit_method`).
- `T2tHandler.tarpit_method: Option<TarpitMethod>` → `tarpit_outcome: Option<TarpitOutcome>`; `resolve_tarpit_method` → `resolve_tarpit_outcome`.
- Each of the ~7 auth-failure call sites (`auth_password`, `auth_keyboard_interactive`, 5 branches in `auth_publickey`): mechanical swap from `if method == Some(FakeShell) {...} if method == Some(SlowAuth) {...}` to matching `TarpitOutcome::Trap{method: FakeShell,..}` / `Trap{method: SlowAuth,..}` / `_ => {}` (covers `None` and `Ban` — both just fall through to the existing `Auth::Reject`, since a Handler callback has no third "just drop" return available).
- `log_auth_failure` gains an `outcome: &TarpitOutcome` param. Derives, uniformly for both `Trap{source,..}` and `Ban{source}`: `tarpit_action` (`"trap"`/`"ban"`/`None`), and from `source` — `Some(Threshold(id))` → `tarpit_threshold_id = Some(id)`; `Some(AdminRule(id))` → `banned_by_ban_rule_id = Some(id)`; `None` → both `None` (the lenient-fallback case). `log_auth_success` untouched (a banned identity never reaches success today, unchanged).

## Web layer

- `crates/tunnel2tunnel-web/src/routes/tarpit.rs`: `TarpitThresholdBody`/`Response` and `CreateBanRuleBody`/`BanRuleResponse` all gain `action: String`; both validators check it's `"trap"`/`"ban"`.
- `ConnLogResponse` (`entities.rs`): add `tarpit_action`, `tarpit_threshold_id`, `banned_by_ban_rule_id`.

## Frontend

- **New `frontend/src/components/MultiButton.vue`**: bootstrap-button-group-style segmented control — a row of attached buttons, no gap, no border-radius except the outer two corners on the end buttons, shared 1px border with no doubled-up border between segments. Props: `options: { value: string; label: string }[]`, emits `select(value)` per click (stateless — it's an action picker, not a toggle holding its own state, since each click here immediately fires a request).
- `AdminConnectionLogsPage.vue`: replace the single "Ban" button with `<MultiButton :options="[{value:'trap',label:'Trap'},{value:'ban',label:'Ban'}]" @select="a => openBanForm(l, a)" />`; `openBanForm`/`confirmBan` thread the chosen `action` into `CreateBanRuleParams`. Add a "Decision" column: shows `tarpitActionLabel[l.tarpit_action]` when set, linking to `/admin/ban-rules#threshold-{id}` (via `tarpit_threshold_id`) or `/admin/ban-rules#rule-{id}` (via `banned_by_ban_rule_id`) — reusing the anchor-link pattern already added for `user_id` → `/admin/users#user-{id}` in the previous session.
- `AdminBanRulesPage.vue`: Threshold Rules table/add-form gets an "Action" column (Trap/Ban select, or a `MultiButton` used as a toggle-select instead of a plain `<select>` — reuse the same component for visual consistency); Ban Rules table also shows its `action` column now that rules carry one; add `:id="`threshold-${t.id}`"`/`:id="`rule-${r.id}`"` row anchors for the connection-log deep-links above.
- `frontend/src/api/admin.ts`: `TarpitThreshold`/`TarpitThresholdParams`/`BanRule`/`CreateBanRuleParams` gain `action: 'trap' | 'ban'`; `ConnLog` gains `tarpit_action`, `tarpit_threshold_id`, `banned_by_ban_rule_id`.
- `frontend/src/labels.ts`: add `tarpitActionLabel`/`tarpitActionOptions` (Trap/Ban), reused by both tables' selects and the log table's Decision column.

## Verification

- `cargo test -p tunnel2tunnel-core -p tunnel2tunnel-ssh -p tunnel2tunnel-web` — update the `tarpit/mod.rs` unit-test `rule()` helper for the new `action` field; add tests: a `"ban"`-action rule trips straight to `Ban`, mixed trap+ban simultaneous trip prefers `Ban`, `resolve_outcome` falls back to `Trap` when the referenced row is missing (both source kinds).
- Run migrations 011/012 locally, re-run `tarpit_models`/`tarpit_e2e` integration suites — must stay green (defaults preserve current behavior for existing rows).
- New `t2t` e2e test: a `"ban"`-action threshold rejects instantly (no slow-auth delay, no fake-shell accept).
- Manually: add a Ban-action threshold rule, verify instant rejection + log row referencing it; click the new `( Trap | Ban )` button on a log row for each side, verify the next attempt is trapped or instantly rejected respectively, with the log row referencing the `ban_rules` row.
