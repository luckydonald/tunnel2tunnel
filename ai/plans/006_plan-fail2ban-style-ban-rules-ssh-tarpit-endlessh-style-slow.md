# Plan: fail2ban-style ban rules + SSH tarpit (endlessh-style + slow-auth + fake shell)

## Context

Server sees constant SSH brute-force noise (same peer_ip, many password attempts). Want fail2ban-like behavior built into the rendezvous server itself: after repeated bad attempts, waste the attacker's time/resources via tarpit techniques (round-robined) instead of/alongside a hard reject, log everything for admin review, and let an admin manage ban rules via the webui. Must never affect a legitimate publickey login (reference sequence in `ai/errors/4.legitimiate.txt`).

Key research finding that shapes the architecture: `russh` 0.61's `run_stream()` unconditionally writes the real `SSH-2.0-...` identification string as its first action, before any `Handler` callback fires. There is no hook to intercept this. So the classic endlessh technique (never send the real ID string, drip endless junk lines forever) **cannot** be done from inside a `Handler` — it requires owning the raw `TcpStream` ourselves, before handing it to russh, for connections we've already decided to tarpit this way. This means `tunnel2tunnel-ssh::start()` moves from `T2tServer::run_on_address(...)` to a hand-rolled `TcpListener::accept()` loop that either (a) spawns a raw banner-drip loop directly on the socket, or (b) hands the socket to `russh::server::run_stream()` as today.

Per user instruction: **all new tarpit logic lives in its own module(s)**, not inline in `crates/tunnel2tunnel-ssh/src/lib.rs`. `lib.rs` keeps only the accept loop, `T2tServer`/`T2tHandler` struct field additions, and thin call-outs into the new module at existing hook points (`log_auth_failure`, `auth_publickey`, `auth_password`, `auth_succeeded`, `channel_open_session`).

## Confirmed decisions (from user)

1. Extend existing `connection_logs` table (don't create a parallel table) — tighten `failure_reason` into a real constrained enum (repo's existing TEXT+CHECK pattern).
2. Ban/tarpit threshold is admin-configurable globally (N failures in window X), applied per peer_ip and per user.
3. Admin-created ban rules (by peer_ip or by user) support optional expiry (`active_until`, NULL = indefinite) — mirrors `entities`/`ssh_keys.valid_until`.
4. Three round-robined tarpit methods: (a) endlessh-style banner drip, (b) slow auth drip-feed, (c) fake interactive shell.

## Module layout (new)

```
crates/tunnel2tunnel-ssh/src/
├── lib.rs              # accept loop, Handler impl — thin hooks only
└── tarpit/
    ├── mod.rs          # TarpitMethod, TarpitState/TarpitEntry, round_robin(), record_failure()/clear_on_success(), Thresholds, settings refresher task, public API surface used by lib.rs
    ├── banner_drip.rs  # raw-socket endless pre-banner drip (method a) — random RFC4253-legal line generator + drip loop
    ├── slow_auth.rs    # slow-auth delay helper (method b) — pure `async fn delay_for(rng) -> Duration`-style + sleep wrapper
    └── fake_shell.rs   # fake interactive shell channel handling (method c) — serve-bogus-prompt loop, reusable from channel_open_session/auth_succeeded
```

All DB access for tarpit stays in `tunnel2tunnel-core` (new `models/ban_rule.rs`, extended `models/connection_log.rs`) — the `tarpit/` module in the ssh crate takes a `PgPool` where needed but contains no SQL itself beyond calling core model functions.

## 1. Migration `migrations/008_tarpit.sql`

- Rename `connection_logs.failure_reason` → `fail_reason`; add `success_reason TEXT`.
- Backfill existing rows (`success_reason='correct login'` where `login_succeeded=true`; `fail_reason=COALESCE(fail_reason,'unspecified failure')` where false) **before** adding constraints.
- `CHECK (fail_reason IS NULL OR fail_reason IN (...))` — values: `password auth not supported`, `unsupported auth method`, `unknown key`, `key expired`, `key expired (valid_until)`, `entity not found`, `entity expired`, `ip blocked by whitelist`, `ip banned`, `user banned`, `rate limited`, `tarpitted - gave up`, `unspecified failure`.
- `CHECK (success_reason IS NULL OR success_reason IN ('correct login', 'admin reset ban'))`.
- `CHECK (num_nonnulls(fail_reason, success_reason) = 1)` — exactly one reason set.
- Drop `login_succeeded`; add `success BOOLEAN GENERATED ALWAYS AS (success_reason IS NOT NULL) STORED` (see Ambiguity #4 below — DB-derived, not app-computed, so the CHECK constraint and the derived column can never drift apart).
- Add `attempted_password TEXT` (raw, intentional — password auth isn't valid here, the attempted value itself is recon signal), `user_id UUID REFERENCES users(id) ON DELETE SET NULL`, `tarpit_method TEXT CHECK (tarpit_method IS NULL OR tarpit_method IN ('banner_drip','slow_auth','fake_shell'))`.
- Indexes: `peer_ip`, `success`, `user_id`, partial index on `fail_reason WHERE NOT NULL`.
- New table `ban_rules`: `id, scope_type TEXT CHECK IN ('peer_ip','user'), peer_ip TEXT, user_id UUID REFERENCES users(id), reason TEXT, active_until TIMESTAMPTZ, created_by UUID NOT NULL REFERENCES users(id), created_at/updated_at` + `CHECK` enforcing exactly one of `peer_ip`/`user_id` matches `scope_type`. `set_timestamps()` trigger. Indexes on `peer_ip`, `user_id`, `active_until`.
- Seed rows in existing `settings` table (reuse it — see Ambiguity #5): `tarpit_threshold_count='5'`, `tarpit_threshold_window_seconds='600'`, `tarpit_enabled='true'`, via `INSERT ... ON CONFLICT (key) DO NOTHING`.

Update `CLAUDE.md` migrations table with the new row.

## 2. `tunnel2tunnel-core` changes

### `models/connection_log.rs`
- Struct gains: `user_id: Option<Uuid>`, `attempted_password: Option<String>`, split `fail_reason`/`success_reason: Option<String>`, `success: bool` (read-only, generated), `tarpit_method: Option<String>`. Drop `login_succeeded`.
- `create(...)` signature grows to take `user_id`, `attempted_password`, `fail_reason`, `success_reason`, `tarpit_method` (replacing the old single `reason`/`login_succeeded` params).
- `set_ended`, `list_for_entity` — unchanged shape.
- New `count_recent_failures_for_peer_ip(pool, peer_ip, since) -> i64` and `count_recent_failures_for_user(pool, user_id, since) -> i64` — `WHERE fail_reason IS NOT NULL AND started_at >= since`.
- New `peer_ip_has_known_good_history(pool, peer_ip) -> bool` — `EXISTS(... WHERE peer_ip=$1 AND success_reason='correct login')`. Drives permanent banner-drip ineligibility (Ambiguity #1).
- New `search(pool, peer_ip, user_id, success, tarpit_method, q, page, page_size) -> (Vec<Self>, i64)` — dynamic `WHERE` with `IS NULL OR` per optional filter, `ILIKE` search across peer_ip/key_fingerprint/attempted_password/fail_reason, `ORDER BY started_at DESC LIMIT/OFFSET`, total via `COUNT(*) OVER()` or a second query.

### New `models/ban_rule.rs`
`BanRule { id, scope_type, peer_ip, user_id, reason, active_until, created_by, ts: Timestamps }` with `list_active(pool)` (`active_until IS NULL OR active_until > NOW()`), `list_all(pool)`, `create(...)`, `delete(pool, id) -> bool`. Register in `models/mod.rs`.

## 3. `tunnel2tunnel-ssh` changes

### `src/tarpit/mod.rs`
```rust
pub enum TarpitMethod { BannerDrip, SlowAuth, FakeShell }
impl TarpitMethod {
    pub fn as_str(self) -> &'static str { ... }               // "banner_drip"/"slow_auth"/"fake_shell"
    pub fn round_robin(trigger_count: u64) -> Self { ... }     // pure fn, unit-testable with no DB/network
}

struct TarpitEntry { fail_count: u32, window_start: Instant, trigger_count: u64,
                     banner_drip_eligible: bool, banned_until: Option<Instant> }

pub type TarpitState = Arc<Mutex<HashMap<String, TarpitEntry>>>; // key: peer_ip or "user:{uuid}"

pub struct Thresholds { pub count: u32, pub window: Duration, pub enabled: bool }

// Pure, DB/network-free — the core of what's unit tested:
pub fn record_failure(map: &mut HashMap<String, TarpitEntry>, key: &str, thresholds: &Thresholds, now: Instant) -> bool;
pub fn clear_auto_counter_on_success(map: &mut HashMap<String, TarpitEntry>, key: &str); // does NOT touch banned_until from admin ban_rules

// Async, DB-touching — thin wrappers used by lib.rs:
pub async fn decide_pre_auth_tarpit(state: &TarpitState, pool: &PgPool, peer_ip: &str) -> Option<TarpitMethod>;
pub async fn record_auth_failure(state: &TarpitState, pool: &PgPool, peer_ip: &str, user_id: Option<Uuid>, thresholds: &Thresholds) -> Option<TarpitMethod>;
pub async fn record_auth_success(state: &TarpitState, peer_ip: &str, user_id: Option<Uuid>);
pub fn spawn_settings_refresher(pool: PgPool, state: TarpitState, thresholds: Arc<Mutex<Thresholds>>); // polls Settings + BanRule::list_active every ~30s
pub fn fail2ban_tarpit_line(peer_ip: &str) -> String; // reuses the *existing* "Failed publickey for invalid user tarpit from <ip> port 0 ssh2" shape — zero fail2ban filter changes needed (Ambiguity #6)
```

### `src/tarpit/banner_drip.rs`
```rust
pub fn random_rfc4253_line(rng: &mut impl Rng) -> String; // never starts with "SSH-", CRLF-terminated, <=255 bytes
pub async fn run(socket: TcpStream, peer_ip: String, pool: PgPool, fail2ban: Option<Arc<String>>);
    // writes a ConnectionLog row (fail_reason="ip banned"/"user banned", tarpit_method="banner_drip"),
    // optional fail2ban line, then loops: write random line, sleep(10s), until write fails or a
    // generous cap (e.g. 6h) elapses; sets ended_at on exit.
```

### `src/tarpit/slow_auth.rs`
```rust
pub async fn delay(); // sleep(3s + jitter), called from lib.rs immediately before an Auth::Reject
                       // in auth_publickey/auth_password when TarpitMethod::SlowAuth was selected —
                       // never called on the accept path to Auth::Accept, so a legit key is never slowed.
```

### `src/tarpit/fake_shell.rs`
```rust
pub async fn serve(handle: Handle, channel_id: ChannelId); // bogus "$ " prompt, echoes input with delay,
    // reuses the existing tokio::select!{ channel.wait() vs interval.tick() } pattern already in
    // lib.rs's real welcome-channel code (~line 442-464) as the template, hangs until client drops.
```

### `lib.rs` changes (thin — logic lives in `tarpit::*`)
- `T2tServer`/`T2tHandler` gain `tarpit: TarpitState` field, wired the same way `server_slots`/`session_registry` already are (constructed once in `start()`, cloned per connection in `new_client`).
- `T2tHandler` gains `tarpit_method: Option<TarpitMethod>` (decided once per connection/attempt) and `fake_shell: bool`.
- `start()`: replace `T2tServer::run_on_address(...)` with a manual `TcpListener::accept()` loop; per accepted connection call `tarpit::decide_pre_auth_tarpit(...)`; if `Some(BannerDrip)`, `tokio::spawn(tarpit::banner_drip::run(...))` directly on the raw socket (never touches russh); otherwise proceed exactly as today via `russh::server::run_stream(config, socket, handler)`. Also call `tarpit::spawn_settings_refresher(...)` once at startup.
- `log_auth_failure`/`log_auth_success` gain `user_id: Option<Uuid>` param, call `tarpit::record_auth_failure`/`record_auth_success`, and pass `attempted_password`/`tarpit_method` through to `ConnectionLog::create`.
- `auth_none`: **no change to counting** — never calls `log_auth_failure`, no row written (every legit client sends this probe; must not count toward any threshold — see Ambiguity #2).
- `auth_password`: log the raw attempted password via the new `attempted_password` param; if `tarpit_method == Some(SlowAuth)`, `tarpit::slow_auth::delay().await` before rejecting.
- `auth_keyboard_interactive`: currently rejects with no log call at all — **add** a `log_auth_failure` call (`"unsupported auth method"`) to close this counting gap.
- `auth_publickey`: per existing rejection branch, thread `user_id` (resolvable once `Entity` is loaded: key-expired, entity-not-found has none, entity-expired, ip-whitelist-blocked all have `user_id` from `entity.user_id`; unknown-key and db-error branches have none) — exact table:
  | branch | counts peer_ip | counts user_id | fail_reason |
  |---|---|---|---|
  | unknown key | yes | no | `unknown key` |
  | db error (key lookup) | no | no | (no row, unchanged) |
  | key soft-deleted | yes | yes | `key expired` |
  | key valid_until expired | yes | yes | `key expired (valid_until)` |
  | entity not found | yes | no | `entity not found` |
  | db error (entity load) | no | no | (no row, unchanged) |
  | entity expired | yes | yes | `entity expired` |
  | ip whitelist blocked | yes | yes | `ip blocked by whitelist` |
  | success | clears auto-counters (not admin bans) for peer_ip+user_id | | `success_reason="correct login"` |

  If `tarpit_method == Some(SlowAuth)`, call `slow_auth::delay().await` right before each `Auth::Reject` above (placed so a genuine `Auth::Accept` path never reaches it). If `tarpit_method == Some(FakeShell)` and the branch is unknown-key or entity-not-found (never for a resolvable-but-expired real entity — that path should still behave like a normal reject, not trap a stale-but-known identity into a fake shell), return `Ok(Auth::Accept)` instead, set `self.fake_shell = true`, leave `self.entity = None`, and log with `fail_reason` matching the trigger (`"ip banned"`/`"user banned"`) plus `tarpit_method="fake_shell"`.
- `auth_succeeded`: add a branch for `self.fake_shell` — still calls `session.handle()` but skips `session_registry`/broadcast, spawns `tarpit::fake_shell` support setup only (nothing to register since there's no real entity).
- `channel_open_session`: guard changes from `let Some(ref authed) = self.entity else { return Ok(false) }` to `if self.entity.is_none() && !self.fake_shell { return Ok(false); }`; branch to `tarpit::fake_shell::serve(...)` when `self.fake_shell`.
- `impl Drop for T2tHandler`: when a fake-shell/banner-drip connection finally drops without ever succeeding, `set_ended` still applies; no new `ConnectionLog` row is written at drop (the row was already written at trigger time with the appropriate `fail_reason`).

## 4. `tunnel2tunnel-web` changes

- New `routes/connection_logs.rs`: `GET /api/admin/connection-logs` (admin-only) with `LogSearchQuery { page, page_size, peer_ip, user_id, success, method, q }`, calls `ConnectionLog::search`, returns `{ items, total, page, page_size }`. `page_size` clamped `[1,200]`. Existing per-entity `GET /api/entities/{id}/logs` route/DTO stays, gains the new fields.
- New `routes/ban_rules.rs`: `GET/POST /api/admin/ban-rules`, `DELETE /api/admin/ban-rules/{id}` (admin-only). `scope_type` validated against `["peer_ip","user"]` the same way `access.rs` validates `subject_type` (literal array + `.contains()`).
- Extend `routes/settings.rs` (or add if it doesn't exist as such): `GET/PUT /api/admin/tarpit-settings` for the three threshold keys.
- Register all new routes in `lib.rs` near the existing route block.

## 5. Frontend changes

- `api/admin.ts`: extend `ConnLog` with new fields, add `TarpitMethod` type, add `searchConnectionLogs`, `listBanRules`/`createBanRule`/`deleteBanRule`, `getTarpitSettings`/`updateTarpitSettings`.
- `labels.ts`: add `tarpitMethodLabel`/`Options`, `banScopeTypeLabel`/`Options`, `failReasonLabel`, `successReasonLabel` — same `Record<Union,string>` + derived `Options` pattern as `subjectTypeLabel`.
- New `pages/AdminConnectionLogsPage.vue`: filter bar (peer_ip, user, success, method, free-text search) + `data-table` (reuse existing `.data-table` styling) + simple Prev/Next pager using `total`/`page_size`.
- New `pages/AdminBanRulesPage.vue`: modeled directly on `EntityDetailPage.vue`'s Access Rules CRUD block — toggleable add-rule form (`scope_type` select → conditional peer_ip/user field, optional `active_until` datetime input, `reason` text), `data-table` with delete button per row. Small "Global Thresholds" card at top for the 3 settings.
- `router/index.ts`: add `/admin/connection-logs`, `/admin/ban-rules` routes, `requiresAuth: true`, self-guard on `auth.user?.is_admin`.
- `components/AppShell.vue`: add nav links gated by `v-if="auth.user?.is_admin"`.

## 6. Tests

- **Pure unit tests, no DB/network** (`crates/tunnel2tunnel-ssh/src/tarpit/mod.rs` and siblings, `#[cfg(test)] mod tests`):
  - `TarpitMethod::round_robin` cycles 0→BannerDrip, 1→SlowAuth, 2→FakeShell, 3→BannerDrip.
  - `record_failure`: N-1 failures within window → not banned; Nth → banned + `trigger_count` bumped; failures outside window don't accumulate.
  - `clear_auto_counter_on_success` resets `fail_count`/`window_start` but leaves a pre-seeded `banned_until` (simulating an admin ban_rules-derived ban) untouched.
  - `banner_drip::random_rfc4253_line`: never starts with `"SSH-"`, CRLF-terminated, length bound.
  - `fail2ban_tarpit_line`: matches the existing `contrib/fail2ban/filter.d/tunnel2tunnel.conf` regex exactly (assert with the `regex` crate against the generated string).
- **DB-touching unit tests** (`#[sqlx::test]` or equivalent) in `tunnel2tunnel-core`:
  - `connection_log.rs`: both-reasons-set / neither-set INSERT fail the CHECK; exactly-one succeeds and `success` generated column matches; `count_recent_failures_for_*` respect the time window; `search` filters/pagination/total correct.
  - `ban_rule.rs`: scope/peer_ip/user_id CHECK enforced both directions; `list_active` excludes expired rows.
- **Web integration tests** (new `crates/tunnel2tunnel-web/tests/ban_rules.rs`): ban-rule CRUD round trip via the router, non-admin gets 403, connection-log search pagination/filter round trip.
- **e2e** (new `crates/t2t/tests/tarpit_e2e.rs`, reusing `tunnel_e2e.rs` fixtures — DB bootstrap, `generate_test_keypair`, `free_port`, server-start-and-poll, `ChildGuard`):
  - **Required legit-login-sequence test**: pre-seed enough prior failures (or a `ban_rules` row) to put a peer_ip/entity well over threshold, then register a real entity+keypair for that *same* identity and run the exact reference sequence (`ai/errors/4.legitimiate.txt`) via a real OpenSSH subprocess. Assert: login completes (no hang) within a short timeout; resulting `connection_logs` row has `success=true`, `success_reason='correct login'`, `tarpit_method IS NULL`.
  - `auth_none`-only probing (connect+probe+disconnect, repeated) never crosses the threshold and never writes a `connection_logs` row.
  - Banner-drip: raw `TcpStream` (not OpenSSH, since OpenSSH would just hang — that's the point) connects after threshold crossed with round-robin landing on `BannerDrip`; asserts repeated non-`"SSH-"` lines arrive and the real ID string never does within a bounded wait.
  - Slow-auth / fake-shell: drive via OpenSSH subprocess with a genuinely unknown key (so real auth legitimately fails) once tarpit method is forced/seeded to each value; assert elapsed time is higher for slow-auth, and for fake-shell assert the client's stdout contains the bogus prompt text and the process must be killed by the test timeout rather than exiting cleanly.

## Design ambiguities resolved (see prior research for full reasoning — kept brief here)

1. **Success doesn't retroactively clear peer_ip/user auto-ban for future connections** (per explicit "not required" in the request) — only guarantees the *currently succeeding* session is never interrupted, which is automatic since a legit key's path to `Auth::Accept` never touches a tarpit branch. Banner-drip specifically becomes permanently ineligible once any success is ever recorded for that peer_ip (can't intercept before SSH-ID otherwise without risking a real device on a shared IP).
2. `auth_none` never logged/counted — it's a mandatory protocol probe every legit client sends.
3. "Ban by user" = ban by `users.id`, reached via `SshKey → Entity.user_id` — only resolvable once a key at least maps to a real (if expired/stale) entity; pure unknown-key scanning never resolves to a user, so it can only ever be IP-banned, not user-banned.
4. `success` is a Postgres `GENERATED ALWAYS AS ... STORED` column, not app-computed — keeps single source of truth with the CHECK constraint, reads back transparently via `sqlx::FromRow`.
5. Reuse the existing (currently unused) `settings` kv table for the 3 threshold values rather than a new mechanism — zero schema ceremony, `Settings::get`/`set` already exist.
6. Tarpit-triggered rejections reuse the *exact existing* fail2ban line shape (sentinel username `tarpit`) so the current filter regex needs no changes and external fail2ban gets defense-in-depth for free.

## Verification

- `cargo build --workspace` and `cargo test --workspace` (unit + `#[sqlx::test]` + the new e2e binary) against a local Postgres 18 per CLAUDE.md's `podman run` instructions.
- `cargo run -p t2t` locally, then manually reproduce the reference legit-login sequence with a real registered entity/key and confirm no delay/hang; separately hammer with a bogus key repeatedly and observe (via `RUST_LOG=tunnel2tunnel_ssh=debug`) the threshold trip, round-robin method selection, and the tarpit taking effect (e.g. `nc localhost 2222` hanging on banner-drip, or `ssh` visibly slowing down / dropping into the fake shell).
- `npm run build` in `frontend/`, then click through the two new admin pages: create/delete a ban rule, browse+filter+paginate connection logs, edit global thresholds.
- Confirm `contrib/fail2ban/filter.d/tunnel2tunnel.conf`'s regex still matches both ordinary failed-publickey lines and the new tarpit sentinel line (unit test covers this, but worth a manual `fail2ban-regex` sanity check too if available).
