All findings below. This was pure investigation (no plan/design, no code written).

## 1. `tarpit_thresholds` and `ban_rules` — schema + model files

**`migrations/009_tarpit_thresholds.sql`** (full, quoted above in tool output):
```sql
CREATE TABLE tarpit_thresholds (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    fail_count      INT NOT NULL CHECK (fail_count > 0),
    window_seconds  BIGINT NOT NULL CHECK (window_seconds > 0),
    enabled         BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```
No `action`/`ban_kind` column exists yet — every rule currently implicitly means "tarpit".

**`crates/tunnel2tunnel-core/src/models/tarpit_threshold.rs`** (full file, quoted above) — `TarpitThreshold { id, fail_count: i32, window_seconds: i64, enabled: bool, ts: Timestamps }`, with `list_all`, `list_enabled`, `create(pool, fail_count, window_seconds, enabled)`, `update(pool, id, fail_count, window_seconds, enabled)`, `delete`. All four DB-touching fns hardcode the column list, so adding an `action` column requires touching every one of these functions' SQL and signatures.

**`ban_rules` table** — in `migrations/008_tarpit.sql` lines 67-88 (quoted above): `id, scope_type ('peer_ip'|'user'), peer_ip, user_id, reason, active_until, created_by (FK users, RESTRICT), created_at, updated_at`, with the mutual-exclusivity CHECK on scope_type/peer_ip/user_id. No action/kind column either — `ban_rules` is currently unconditionally "ban" (never routes to tarpit at all; see §5).

**`crates/tunnel2tunnel-core/src/models/ban_rule.rs`** (full file, quoted above) — `BanRule { id, scope_type: String, peer_ip: Option<String>, user_id: Option<Uuid>, reason: Option<String>, active_until: Option<OffsetDateTime>, created_by: Uuid, ts: Timestamps }`, with `list_active`, `list_all`, `create(pool, scope_type, peer_ip, user_id, reason, active_until, created_by)`, `delete`.

## 2. Tarpit engine — `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs` (full file quoted above, 469 lines)

Key pieces with line numbers:
- `TarpitMethod` enum + `round_robin` (lines 26-51): only 3 variants (`BannerDrip`/`SlowAuth`/`FakeShell`), `round_robin(trigger_count)` cycles mod 3 — there is no "hard ban"/"reject" variant.
- `BanUntil` (lines 53-68): `NotBanned` / `At(Instant)` / `Forever` — pure timing, method-agnostic.
- `TarpitEntry` (lines 70-98): `tallies: HashMap<Uuid, (u32, Instant)>` keyed by `tarpit_thresholds.id`, `trigger_count: u64`, `banner_drip_eligible: bool`, `banned: BanUntil`. No field recording *which* rule tripped, nor *why* (auto vs admin), nor an action kind.
- `ThresholdConfig` (lines 114-124): `{ enabled: bool, rules: Vec<TarpitThreshold> }` — plain list of rows, no distinction of action per rule.
- `record_failure` (lines 135-170): iterates `rules`, bumps each rule's own tally, and on any rule reaching `fail_count` sets `tripped_window = max(existing, window)` and later sets `entry.banned = BanUntil::At(now + window)`, `entry.trigger_count += 1`. **Crucially it discards *which* rule id tripped** — only the longest window survives, no reference to `rule.id` is kept on the entry after tripping. This is the exact place a "Trap vs Ban" action per rule would need new logic (currently there's no way to know which of possibly several tripped rules should decide the action, nor to stash "this ban came from rule X" for later logging).
- `clear_fail_tally_on_success` (172-182).
- `decide_pre_auth_tarpit` (219-251): only returns `Option<TarpitMethod>` (i.e., `None` or one of the 3 methods) — never a "just close the socket" signal. If banned, always calls `TarpitMethod::round_robin(trigger_count)`, with a banner-drip→slow-auth downgrade for entities with known-good history. There's no branch that skips tarpit and just drops/rejects.
- `decide_in_auth_tarpit` (257-287): same shape — `Option<TarpitMethod>`, banned→always round-robins (mapping `BannerDrip`→`SlowAuth` since real SSH banner has already gone out). Again no "hard reject" path.
- `spawn_settings_refresher` / `refresh_once` (295-346): merges `ban_rules` into the same `TarpitState` map used for threshold-tripped bans (see §5 below) — `ban_rules` entries just set `entry.banned = Forever/At(...)` with no marker distinguishing them from auto-tripped bans; both are consumed identically by `decide_*_tarpit`.
- `fail2ban_tarpit_line` (352-356).
- Unit tests (358-469) cover `round_robin`, `record_failure` threshold/window/independent-rules behavior, success-clears-tally-not-ban, banner-drip-eligibility flip, and the fail2ban line regex.

## 3. `connection_logs` schema history + full model

- `migrations/004_connection_logs.sql` (full, quoted above): original columns `id, entity_id, peer_ip, key_fingerprint, login_succeeded, failure_reason, ssh_flags, ports_requested, started_at, ended_at, created_at, updated_at`.
- `migrations/008_tarpit.sql` lines 6-61 (quoted above): renames `failure_reason`→`fail_reason`; adds `success_reason`, backfills, adds CHECK constraints on both reason columns' allowed values, adds XOR check `num_nonnulls(fail_reason, success_reason) = 1`; drops `login_succeeded`, adds generated `success BOOLEAN`; adds `attempted_password`, `user_id UUID REFERENCES users(id) ON DELETE SET NULL`, and **`tarpit_method TEXT CHECK (tarpit_method IS NULL OR tarpit_method IN ('banner_drip','slow_auth','fake_shell'))`** (lines 55-56) — this CHECK constraint would need extending/rethinking for auto-ban/admin-ban markers. Indexes on peer_ip, success, user_id, fail_reason.
- `migrations/010_connection_log_attempted_username.sql` (full, quoted above): adds `attempted_username TEXT`.

**`crates/tunnel2tunnel-core/src/models/connection_log.rs`** (full file quoted above). Struct (lines 11-30):
```rust
pub struct ConnectionLog {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub attempted_password: Option<String>,
    pub attempted_username: Option<String>,
    pub fail_reason: Option<String>,
    pub success_reason: Option<String>,
    pub success: bool,
    pub tarpit_method: Option<String>,
    pub ssh_flags: Option<String>,
    pub ports_requested: Option<String>,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    pub ts: Timestamps,
}
```
`create()` (lines 34-69) takes 10 positional args ending `tarpit_method: Option<&str>, started_at` — no field/param currently exists for "which admin banned this" or "which threshold rule auto-banned this". Also relevant: `count_recent_failures_for_peer_ip`/`_for_user` (96-130), `peer_ip_has_known_good_history` (137-149), `search` (152-211, filters on `tarpit_method` exact match via `$4::text`), `entity_status`/`entity_statuses` (213-255).

## 4. Admin "Ban" button flow, `create_ban_rule`, `AdminUser` extractor

**Frontend** — `frontend/src/pages/AdminConnectionLogsPage.vue`:
- `openBanForm`/`confirmBan`/state (lines 61-92, quoted above). Note `confirmBan` builds `CreateBanRuleParams` purely from the `ConnLog` row — `user_id` if present else `peer_ip` — with `reason`/`active_until`; it never sends any "which admin" info (that's inferred server-side from the session in `create_ban_rule`).
- Ban button template: line 157: `<button v-if="!l.success" class="btn-secondary btn-ban" @click="openBanForm(l)">Ban</button>`.
- Modal template lines 172-193.

**`frontend/src/api/admin.ts`**: `createBanRule` (lines 183-187) — `POST /api/admin/ban-rules` with `CreateBanRuleParams` (lines 58-64: `scope_type, peer_ip?, user_id?, reason?, active_until?`) — no admin identity sent from client.

**Backend** — `crates/tunnel2tunnel-web/src/routes/tarpit.rs`:
- `CreateBanRuleBody` (lines 114-122): `scope_type, peer_ip, user_id, reason, active_until` — no action/kind field.
- `validate_create_ban_rule` (126-140, quoted above) — checks scope_type is `peer_ip`/`user` and matching field present.
- `create_ban_rule` (142-162):
```rust
pub async fn create_ban_rule(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateBanRuleBody>,
) -> Result<(StatusCode, Json<BanRuleResponse>), WebError> {
    validate_create_ban_rule(&body)?;
    let rule = BanRule::create(
        &state.db, &body.scope_type, body.peer_ip.as_deref(), body.user_id,
        body.reason.as_deref(), body.active_until, admin.id,
    ).await.map_err(WebError::Core)?;
    Ok((StatusCode::CREATED, Json(BanRuleResponse::from(rule))))
}
```
`admin.id` (the currently-logged-in admin's user id) is already captured as `ban_rules.created_by` — this is the existing mechanism that would double as "which admin performed an Admin Ban", though today `ban_rules` rows aren't linked back to a specific `connection_logs` row nor is there a `connection_logs.banned_by_admin_id`/similar.

**`AdminUser` extractor** — `crates/tunnel2tunnel-web/src/extractors.rs` (full file quoted above, lines 41-51):
```rust
impl FromRequestParts<AppState> for AdminUser {
    type Rejection = WebError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin { return Err(WebError::Forbidden); }
        Ok(AdminUser(user))
    }
}
```
`AdminUser(User)` wraps the full `User` struct (`crates/tunnel2tunnel-core/src/models/user.rs` lines 8-19): `id: Uuid, username: String, email: Option<String>, password_hash (skip), is_admin: bool, is_locked: bool, description: Option<String>, ts`. So any route handler destructuring `AdminUser(admin)` has `admin.id` and `admin.username` (and `is_admin`/`is_locked`/etc.) directly available — no extra lookup needed to record "which admin" performed an action.

## 5. Where "banned" unconditionally means "run a tarpit method"

- `refresh_once` (mod.rs 305-346, quoted above): merges every active `ban_rules` row into the *same* `TarpitState` map (`map.entry(key)...banned = banned` at line 344) that `record_failure` also writes to for threshold-triggered bans. There is **no field distinguishing an admin-created `ban_rules` ban from an auto-triggered threshold ban** once it's in `TarpitEntry.banned` — both simply set `BanUntil::{At,Forever}` on the same `banned: BanUntil` field.
- `decide_pre_auth_tarpit` (219-251) and `decide_in_auth_tarpit` (257-287): both check only `e.is_banned(now)` (a boolean derived from `BanUntil`) — if true, they *always* proceed to `TarpitMethod::round_robin(trigger_count)` and return `Some(method)`. There is no branch anywhere that says "if this ban is a hard Ban (not Trap), return `None`+a separate reject signal instead of a `TarpitMethod`". This is precisely where a `Trap`-vs-`Ban` action field would need to fork the logic: both functions' return types (`Option<TarpitMethod>`) and their sole callers in `lib.rs` (§6) only ever branch on `TarpitMethod` variants, never on an "immediately reject" outcome.
- Note also: `trigger_count`/`banner_drip_eligible`/`tallies` are per-`TarpitEntry` (keyed by peer_ip or `user:{uuid}`), not per-rule at the "is banned" check point — so even independent of Trap/Ban, there's currently no way for `decide_*_tarpit` to know *which* `TarpitThreshold` id caused the active ban (that information is discarded inside `record_failure`, see §2).

## 6. `T2tHandler`/`T2tServer` accept loop + auth handlers — `crates/tunnel2tunnel-ssh/src/lib.rs`

**Pre-auth (raw socket) branch** — inside `start()`'s accept loop, lines 122-156 (quoted above):
```rust
let peer_ip = addr.ip().to_string();
let pre_auth_tarpit =
    tarpit::decide_pre_auth_tarpit(&server.tarpit, &server.pool, &peer_ip).await;

if pre_auth_tarpit == Some(TarpitMethod::BannerDrip) {
    ...
    tokio::spawn(tarpit::banner_drip::run(socket, peer_ip, pool, fail2ban, "ip banned"));
    continue;
}

let mut handler = server.new_client(Some(addr));
handler.tarpit_method = pre_auth_tarpit;
let cfg = russh_config.clone();
tokio::spawn(async move {
    match russh::server::run_stream(cfg, socket, handler).await { ... }
});
```
This is the **only place with the raw `TcpStream` still in hand**, before `russh::server::run_stream` is ever called. A hard-reject-at-accept-time branch (for a peer_ip-level Ban action) would need to be inserted here, as a new `if`/`match` arm on `pre_auth_tarpit` (or a new returned enum) *before* `server.new_client`/`run_stream` — i.e. just `drop(socket)` (or write nothing and close) and `continue`, parallel to the existing `BannerDrip` arm. Currently `pre_auth_tarpit` can only be `None` or one of the 3 `TarpitMethod` variants, so this requires either widening the return type of `decide_pre_auth_tarpit` or adding a sibling check.

**In-auth (already inside `Handler` callbacks) branches** — `T2tHandler::resolve_tarpit_method` (lines 229-240, quoted above) is the single call site for `decide_in_auth_tarpit`, invoked from every auth callback (`auth_password` line 351, `auth_keyboard_interactive` line 381, `auth_publickey`'s several failure branches at lines 432, 458, 484, 496, 509, 527). Every one of these branches follows the same pattern (e.g. `auth_password`, lines 343-366, quoted above):
```rust
let method = self.resolve_tarpit_method(None).await;
self.log_auth_failure(...).await;

if method == Some(TarpitMethod::FakeShell) {
    self.fake_shell = true;
    return Ok(Auth::Accept);
}
if method == Some(TarpitMethod::SlowAuth) {
    tarpit::slow_auth::delay().await;
}
Ok(Auth::Reject { proceed_with_methods: Some(publickey_only()), partial_success: false })
```
Since `russh` handler callbacks must return `Result<Auth, Self::Error>` (either `Auth::Accept` or `Auth::Reject{..}` — there is no third "just drop the TCP connection" variant available from inside a `Handler` callback), a hard-Ban-in-auth branch cannot literally "close the socket instantly" the way the pre-auth branch can; it can only fall through to the existing `Ok(Auth::Reject{...})` path immediately, skipping the `FakeShell`/`SlowAuth` special-casing (i.e., an extra `if method == Some(TarpitMethod::HardBan) { /* skip everything below, straight to Reject */ }`-shaped branch would need inserting into each of the ~7 call sites listed above, or a refactor consolidating them). This is exactly the fork point mentioned in `decide_in_auth_tarpit`'s doc comment (`"banner drip is never selectable here"`) — a Ban action would need a similarly-documented restriction/rationale.

**Auth handler full text** for reference (also quoted above): `auth_none` (330-341, always publickey-only reject, doesn't consult tarpit at all), `auth_password` (343-366), `auth_keyboard_interactive` (368-396), `auth_publickey_offered` (398-413, always `Auth::Accept` — probe only), `auth_publickey` (415-547, the long key/entity/expiry/whitelist validation chain with 6 failure branches all following the same `resolve_tarpit_method`→`log_auth_failure`→branch-on-method→`Auth::Reject` shape, plus the final success path lines 536-546 calling `log_auth_success`/`record_auth_success` and setting `self.entity`).

`log_auth_failure` (242-288, quoted above) is the single place `ConnectionLog::create(...)` is called for failures, passing `self.tarpit_method.map(TarpitMethod::as_str)` as the `tarpit_method` column value — this is where a future "auto-ban"/"admin-ban" marker and "which rule/admin" reference would need to be threaded through, since it's the sole write path for connection_logs rows on the failure side. `log_auth_success` (290-324) is the equivalent for the success path (always writes `tarpit_method: None`).

## 7. `AdminBanRulesPage.vue` — full content

Re-read fresh; full file quoted above (`frontend/src/pages/AdminBanRulesPage.vue`, 355 lines). Key structural facts for the "action column" extension:
- **Threshold Rules table** template lines 211-232: header row (line 213-219) is `Failed attempts | Window | Enabled | (delete)`; body row (222-229) renders `t.fail_count`, `formatWindow(t.window_seconds)`, an `<input type="checkbox">` bound to `t.enabled` calling `handleToggleThreshold(t)` on change, and a delete button. This is the table that would need a new "Action" column (Trap/Ban) plus a corresponding form control in the "Add rule" form (lines 197-209, currently only `fail_count`/`window_seconds` inputs) and in `NewThreshold`/`blankThreshold` (lines 111-112) and `handleAddThreshold`/`handleToggleThreshold` (116-145), which currently only send `{fail_count, window_seconds, enabled}` to `adminApi.createTarpitThreshold`/`updateTarpitThreshold`.
- **Ban Rules ("Rules") table** template lines 259-283: columns `Scope | Target | Reason | Active until | (delete)` — this table has no action concept at all (as noted, `ban_rules` today is unconditionally "ban", never tarpit).
- Imports `banScopeTypeLabel, banScopeTypeOptions` from `@/labels` (line 5) — a sibling labels file exists that would presumably need analogous `trapOrBanLabel`/`trapOrBanOptions` entries (not yet inspected — the user didn't ask for `@/labels` contents, note this as a gap if the design phase needs it).