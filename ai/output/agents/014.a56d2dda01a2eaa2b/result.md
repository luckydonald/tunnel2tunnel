# Implementation Plan: Fail2ban-Style Banning + SSH Tarpit for tunnel2tunnel

## 0. Executive summary of the hardest design problem, up front

`russh::server::run_stream` (called internally by `Server::run_on_address`/`run_on_socket`) **unconditionally writes the real `SSH-2.0-...` identification string as its first action**, before any `Handler` callback ever fires:

```rust
// russh-0.61.2/src/server/mod.rs, run_stream()
let mut write_buffer = SSHBuffer::new();
write_buffer.send_ssh_id(&config.as_ref().server_id);
map_err!(stream.write_all(&write_buffer.buffer[..]).await)?;
```

There is **no `Handler` hook that runs before this**. This means the classic endlessh technique (dribble random RFC4253 "other lines of data" forever, *never* sending the real ID string) **cannot be implemented on top of the `Handler`/`Server` trait at all** — it requires bypassing `russh` entirely for connections we've already decided to drip-feed, and operating directly on the raw `tokio::net::TcpStream` ourselves, deciding *before* handing the socket to russh.

Consequently `tunnel2tunnel-ssh::start()` can no longer use `T2tServer::run_on_address(...)`. It must run its own `TcpListener::accept()` loop (mirroring what `run_on_socket` does internally, which is simple enough to copy) so it can consult ban/tarpit state per accepted connection and choose between:
- **normal path** → build a `T2tHandler` via `server.new_client(...)` and call `russh::server::run_stream(config, socket, handler)` as usual (this is where methods b/c — slow-auth-drip and fake-shell — are applied, from inside the real `Handler` callbacks), or
- **banner-drip path** → spawn a raw loop that owns the `TcpStream` directly and never touches russh.

This is the single biggest architectural change and is called out as Ambiguity/Resolution #1 below.

---

## 1. Migration: `migrations/008_tarpit.sql`

```sql
-- ============================================================================
-- 1. Extend connection_logs: tighten failure_reason into a constrained enum,
--    add the "why it succeeded" counterpart, derive `success`, and add the
--    new tarpit-related columns.
-- ============================================================================

ALTER TABLE connection_logs RENAME COLUMN failure_reason TO fail_reason;
ALTER TABLE connection_logs ADD COLUMN success_reason TEXT;

-- Backfill BEFORE adding the mutual-exclusivity CHECK so existing rows satisfy it.
UPDATE connection_logs
   SET success_reason = 'correct login',
       fail_reason    = NULL
 WHERE login_succeeded = true;

UPDATE connection_logs
   SET fail_reason = COALESCE(fail_reason, 'unspecified failure')
 WHERE login_succeeded = false;

ALTER TABLE connection_logs
  ADD CONSTRAINT connection_logs_fail_reason_check
  CHECK (fail_reason IS NULL OR fail_reason IN (
    'password auth not supported',   -- auth_password always hits this
    'unsupported auth method',       -- keyboard-interactive, others
    'unknown key',
    'key expired',                   -- ssh_key soft-deleted (deleted_at)
    'key expired (valid_until)',
    'entity not found',
    'entity expired',
    'ip blocked by whitelist',
    'ip banned',                     -- admin ban_rules (scope_type='peer_ip')
    'user banned',                   -- admin ban_rules (scope_type='user')
    'rate limited',                  -- threshold auto-ban, not yet an admin ban_rules row
    'tarpitted - gave up',           -- connection dropped mid-tarpit (any method)
    'unspecified failure'            -- backfill-only value for pre-migration rows
  ));

ALTER TABLE connection_logs
  ADD CONSTRAINT connection_logs_success_reason_check
  CHECK (success_reason IS NULL OR success_reason IN (
    'correct login',
    'admin reset ban'   -- reserved for a future "admin manually cleared this
                         -- connection's ban and let it through" flow; not
                         -- wired to any code path in v1, kept so the enum
                         -- doesn't need another migration when that lands
  ));

-- Exactly one of the two reason columns must be non-NULL. Postgres's built-in
-- num_nonnulls() is exactly this "exactly one of N is non-null" idiom.
ALTER TABLE connection_logs
  ADD CONSTRAINT connection_logs_reason_xor_check
  CHECK (num_nonnulls(fail_reason, success_reason) = 1);

-- Derive `success` from success_reason. GENERATED ALWAYS AS ... STORED (not an
-- app-computed field) — see "Ambiguity #4" below for why.
ALTER TABLE connection_logs DROP COLUMN login_succeeded;
ALTER TABLE connection_logs
  ADD COLUMN success BOOLEAN GENERATED ALWAYS AS (success_reason IS NOT NULL) STORED;

-- New columns needed by this feature.
ALTER TABLE connection_logs ADD COLUMN attempted_password TEXT;        -- raw, intentional (recon value)
ALTER TABLE connection_logs ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD COLUMN tarpit_method TEXT
  CHECK (tarpit_method IS NULL OR tarpit_method IN ('banner_drip', 'slow_auth', 'fake_shell'));

CREATE INDEX connection_logs_peer_ip_idx  ON connection_logs (peer_ip);
CREATE INDEX connection_logs_success_idx  ON connection_logs (success);
CREATE INDEX connection_logs_user_id_idx  ON connection_logs (user_id);
CREATE INDEX connection_logs_fail_reason_idx ON connection_logs (fail_reason) WHERE fail_reason IS NOT NULL;

-- ============================================================================
-- 2. Admin-managed ban/tarpit rules — by peer_ip OR by user, optional expiry.
-- ============================================================================

CREATE TABLE ban_rules (
    id            UUID PRIMARY KEY DEFAULT uuidv7(),
    scope_type    TEXT NOT NULL CHECK (scope_type IN ('peer_ip', 'user')),
    peer_ip       TEXT,
    user_id       UUID REFERENCES users(id) ON DELETE CASCADE,
    reason        TEXT,
    active_until  TIMESTAMPTZ,       -- NULL = indefinite, mirrors entities/ssh_keys.valid_until
    created_by    UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
      (scope_type = 'peer_ip' AND peer_ip IS NOT NULL AND user_id IS NULL) OR
      (scope_type = 'user'    AND user_id IS NOT NULL AND peer_ip IS NULL)
    )
);
CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON ban_rules
  FOR EACH ROW EXECUTE FUNCTION set_timestamps();
CREATE INDEX ban_rules_peer_ip_idx ON ban_rules (peer_ip) WHERE peer_ip IS NOT NULL;
CREATE INDEX ban_rules_user_id_idx ON ban_rules (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX ban_rules_active_until_idx ON ban_rules (active_until);

-- ============================================================================
-- 3. Admin-configurable global thresholds — reuse the existing settings kv table
--    (already exists, already seeded with fail2ban_log_path/signup_enabled,
--    currently under-used — reviving it here is the smallest-footprint option
--    and needs no new table/migration ceremony; see Ambiguity #5).
-- ============================================================================

INSERT INTO settings (key, value) VALUES
  ('tarpit_threshold_count',         '5'),     -- N failed attempts...
  ('tarpit_threshold_window_seconds','600'),   -- ...within window X (seconds)
  ('tarpit_enabled',                 'true')   -- admin kill-switch for the whole feature
ON CONFLICT (key) DO NOTHING;
```

Update `CLAUDE.md`'s migrations table with the `008_tarpit.sql` row (small doc edit, not code, still worth doing alongside).

---

## 2. `tunnel2tunnel-core` changes

### 2.1 `crates/tunnel2tunnel-core/src/models/connection_log.rs` (rewrite)

```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ConnectionLog {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub attempted_password: Option<String>,
    pub fail_reason: Option<String>,
    pub success_reason: Option<String>,
    pub success: bool,                 // generated column, read-only
    pub tarpit_method: Option<String>,
    pub ssh_flags: Option<String>,
    pub ports_requested: Option<String>,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    pub ts: Timestamps,
}
```

New/changed functions:

```rust
impl ConnectionLog {
    // Replaces the old create(); reason is now split fail/success, plus password/tarpit_method.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        entity_id: Option<Uuid>,
        user_id: Option<Uuid>,
        peer_ip: Option<&str>,
        key_fingerprint: Option<&str>,
        attempted_password: Option<&str>,
        fail_reason: Option<&str>,
        success_reason: Option<&str>,
        tarpit_method: Option<&str>,
        started_at: OffsetDateTime,
    ) -> Result<Self, CoreError> { /* INSERT ... RETURNING * ; unchanged shape otherwise */ }

    pub async fn set_ended(pool: &PgPool, id: Uuid) -> Result<(), CoreError> { /* unchanged */ }

    pub async fn list_for_entity(pool: &PgPool, entity_id: Uuid, limit: i64)
        -> Result<Vec<Self>, CoreError> { /* unchanged, still used by per-entity view */ }

    /// Count fail-reason rows for `peer_ip` since `since` — used by the ban-threshold engine.
    /// Excludes rows with fail_reason IN the "not a real credential rejection" set
    /// (there currently are none such — see Ambiguity #2 for why `auth_none` never
    /// gets a row at all rather than being filtered out here).
    pub async fn count_recent_failures_for_peer_ip(
        pool: &PgPool, peer_ip: &str, since: OffsetDateTime,
    ) -> Result<i64, CoreError> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM connection_logs \
             WHERE peer_ip = $1 AND fail_reason IS NOT NULL AND started_at >= $2",
        ).bind(peer_ip).bind(since).fetch_one(pool).await.map_err(CoreError::Sqlx)
    }

    /// Count fail-reason rows resolvable to a concrete user_id — only fires for
    /// reasons that got as far as resolving ssh_key → entity → user_id (see
    /// Ambiguity #3 for exactly which fail_reason values these are).
    pub async fn count_recent_failures_for_user(
        pool: &PgPool, user_id: Uuid, since: OffsetDateTime,
    ) -> Result<i64, CoreError> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM connection_logs \
             WHERE user_id = $1 AND fail_reason IS NOT NULL AND started_at >= $2",
        ).bind(user_id).bind(since).fetch_one(pool).await.map_err(CoreError::Sqlx)
    }

    /// True if this peer_ip has ever had a genuinely successful login — used to
    /// exclude an IP from banner-drip eligibility forever (see Ambiguity #1/#2).
    pub async fn peer_ip_has_known_good_history(pool: &PgPool, peer_ip: &str)
        -> Result<bool, CoreError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM connection_logs \
             WHERE peer_ip = $1 AND success_reason = 'correct login')",
        ).bind(peer_ip).fetch_one(pool).await.map_err(CoreError::Sqlx)
    }

    /// Paginated/filtered/searched admin browser query.
    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        pool: &PgPool,
        peer_ip: Option<&str>,
        user_id: Option<Uuid>,
        success: Option<bool>,
        tarpit_method: Option<&str>,
        q: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<Self>, i64), CoreError> {
        // WHERE ($1::text IS NULL OR peer_ip = $1)
        //   AND ($2::uuid IS NULL OR user_id = $2)
        //   AND ($3::bool IS NULL OR success = $3)
        //   AND ($4::text IS NULL OR tarpit_method = $4)
        //   AND ($5::text IS NULL OR peer_ip ILIKE '%' || $5 || '%'
        //                          OR key_fingerprint ILIKE '%' || $5 || '%'
        //                          OR attempted_password ILIKE '%' || $5 || '%'
        //                          OR fail_reason ILIKE '%' || $5 || '%')
        // ORDER BY started_at DESC LIMIT $6 OFFSET $7
        // + a COUNT(*) OVER() window column (or a second COUNT query) for `total`
    }
}
```

### 2.2 New `crates/tunnel2tunnel-core/src/models/ban_rule.rs`

```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct BanRule {
    pub id: Uuid,
    pub scope_type: String,           // 'peer_ip' | 'user'
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub active_until: Option<OffsetDateTime>,
    pub created_by: Uuid,
    #[sqlx(flatten)] #[serde(flatten)]
    pub ts: Timestamps,
}

impl BanRule {
    pub async fn list_active(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        // WHERE active_until IS NULL OR active_until > NOW()
        // — this is the query the in-process cache refresher polls (see 3.2).
    }
    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>, CoreError> { /* admin CRUD listing, incl. expired */ }
    pub async fn create(pool: &PgPool, scope_type: &str, peer_ip: Option<&str>,
        user_id: Option<Uuid>, reason: Option<&str>, active_until: Option<OffsetDateTime>,
        created_by: Uuid) -> Result<Self, CoreError> { /* INSERT ... RETURNING * */ }
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> { /* DELETE */ }
}
```

### 2.3 `crates/tunnel2tunnel-core/src/models/mod.rs` — add `pub mod ban_rule;`

---

## 3. `tunnel2tunnel-ssh` changes (`crates/tunnel2tunnel-ssh/src/lib.rs`)

### 3.1 New shared state type, threaded exactly like `server_slots`/`session_registry`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TarpitMethod { BannerDrip, SlowAuth, FakeShell }

impl TarpitMethod {
    fn as_str(self) -> &'static str {
        match self { Self::BannerDrip => "banner_drip", Self::SlowAuth => "slow_auth", Self::FakeShell => "fake_shell" }
    }
    fn round_robin(trigger_count: u64) -> Self {
        match trigger_count % 3 { 0 => Self::BannerDrip, 1 => Self::SlowAuth, _ => Self::FakeShell }
    }
}

struct TarpitEntry {
    fail_count: u32,
    window_start: Instant,
    trigger_count: u64,           // how many times threshold has been crossed; drives round-robin
    banner_drip_eligible: bool,   // false once peer_ip has ANY known-good login (Ambiguity #1/#2)
    banned_until: Option<Instant>,
}

type TarpitState = Arc<Mutex<HashMap<String, TarpitEntry>>>; // key: peer_ip, or "user:{uuid}"
```

`T2tServer`/`T2tHandler` both gain a `tarpit: TarpitState` field and a `thresholds: Arc<ArcSwap<Thresholds>>` (or simpler: re-read from `settings` on a periodic background refresh task, storing into an `Arc<Mutex<Thresholds>>` — reuse the `tokio::sync::Mutex` pattern already used elsewhere rather than pulling in a new dependency like `arc-swap`).

A background task (spawned once in `start()`, alongside the accept loop) periodically (every ~30s):
1. Re-reads `tarpit_threshold_count` / `tarpit_threshold_window_seconds` / `tarpit_enabled` from `Settings::get` and updates the shared `Thresholds`.
2. Re-reads `BanRule::list_active()` and merges into the `TarpitState` map (setting `banned_until` for admin-created bans, keyed by `peer_ip` directly, or by `user:{uuid}` for user-scoped rows — resolved via `Entity.user_id`, see Ambiguity #3), so admin webui edits take effect without a restart.
3. Evicts stale entries whose window has expired and who have no active ban, to keep the map bounded.

### 3.2 Restructured `start()` — custom accept loop (replaces `run_on_address`)

```rust
pub async fn start(config: SshConfig, pool: PgPool) -> Result<()> {
    let key = load_or_generate_host_key(...)?;
    let russh_config = Arc::new(Config { keys: vec![key], ..Config::default() });

    let server_slots: ServerSlots = Arc::new(Mutex::new(HashMap::new()));
    let session_registry: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
    let tarpit: TarpitState = Arc::new(Mutex::new(HashMap::new()));
    let fail2ban = config.fail2ban_log_path.map(Arc::new);

    spawn_tarpit_settings_refresher(pool.clone(), tarpit.clone()); // background poll loop

    let mut server = T2tServer { pool: pool.clone(), server_slots, session_registry, fail2ban, tarpit: tarpit.clone() };

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.ssh_port)).await?;
    tracing::info!(port = config.ssh_port, "SSH server listening");

    loop {
        let (socket, addr) = listener.accept().await?;
        let peer_ip = addr.map(...)...to_string();

        match decide_pre_auth_tarpit(&tarpit, &pool, &peer_ip).await {
            Some(TarpitMethod::BannerDrip) => {
                let pool = pool.clone();
                let fail2ban = server.fail2ban.clone();
                tokio::spawn(run_banner_drip_tarpit(socket, peer_ip, pool, fail2ban));
            }
            _ => {
                // Normal path — slow_auth/fake_shell (if selected) are applied
                // *inside* the Handler, not here.
                let handler = server.new_client(Some(addr));
                let config = russh_config.clone();
                tokio::spawn(async move {
                    match russh::server::run_stream(config, socket, handler).await {
                        Ok(session) => { let _ = session.await; }
                        Err(e) => tracing::debug!(err = %e, "SSH: connection setup failed"),
                    }
                });
            }
        }
    }
}
```

`decide_pre_auth_tarpit` only ever returns `BannerDrip` (never `SlowAuth`/`FakeShell` — those are per-auth-attempt decisions made later, once inside `auth_publickey`/`auth_password`, because they need to inspect the *method being used*, not just the IP). It:
1. Locks `tarpit`, checks `banned_until` and whether the map has crossed the threshold for `peer_ip`.
2. If banned **and** `banner_drip_eligible` **and** `TarpitMethod::round_robin(trigger_count) == BannerDrip` → return `Some(BannerDrip)`.
3. Otherwise (not banned, or banned but not eligible for banner-drip, or round-robin picked a different method) → `None` — proceed normally; `auth_publickey`/`auth_password` will apply the method internally if still warranted.

### 3.3 Banner-drip tarpit implementation (method a)

```rust
/// RFC4253 §4.2: before the real "SSH-2.0-..." line, a server MAY send other
/// lines of data, CRLF-terminated, none of which may start with "SSH-". Real
/// clients (and endlessh-vulnerable scanners) must tolerate and skip these,
/// but do so by *blocking on read* waiting for the eventual real ID line —
/// which we simply never send.
async fn run_banner_drip_tarpit(
    mut socket: TcpStream,
    peer_ip: String,
    pool: PgPool,
    fail2ban: Option<Arc<String>>,
) {
    let log = ConnectionLog::create(&pool, None, None, Some(&peer_ip), None, None,
        Some("ip banned"), None, Some("banner_drip"), now).await.ok();
    if let Some(path) = &fail2ban { append_to_file(path, &fail2ban_tarpit_line(&peer_ip)).await.ok(); }

    let mut rng = ...;
    loop {
        let line = random_rfc4253_line(&mut rng); // e.g. random base64-ish junk, <=255 bytes, no leading "SSH-"
        if socket.write_all(line.as_bytes()).await.is_err() { break; }
        tokio::time::sleep(Duration::from_secs(10)).await; // slow drip — configurable constant, not per-conn state
        // Optional cap, e.g. 6 hours, then close — avoids unbounded fd/task growth under a large scan.
    }
    if let Some(id) = log { ConnectionLog::set_ended(&pool, id.id).await.ok(); }
}
```

### 3.4 Slow-auth drip-feed (method b) — inside `auth_publickey`/`auth_password`

At the top of `auth_publickey`/`auth_password`, after the existing `none`-probe carve-out (see Ambiguity #2), resolve `self.tarpit_method_for_this_attempt` (computed once at handler construction time from peer_ip, refined again once the offered key's fingerprint doesn't match a known key — for `auth_password` it's simpler, always resolvable from peer_ip alone since password auth is never legitimate). If `SlowAuth` is selected:

```rust
if let Some(TarpitMethod::SlowAuth) = self.tarpit_method {
    tokio::time::sleep(Duration::from_secs(3 + rand_jitter(0..=4))).await;
}
```
placed immediately **before** each `return Ok(Auth::Reject { ... })` in the existing rejection branches (unknown key, entity not found, etc.) — never before a genuine `Auth::Accept`. Because the delay is only reachable from code paths that already concluded "reject", a legitimate key can never be slowed down: its flow reaches `Ok(Auth::Accept)` without ever touching the delay branch.

### 3.5 Fake interactive shell (method c)

If `self.tarpit_method == Some(TarpitMethod::FakeShell)` **and** the attempt is on a path that would otherwise reject (unknown key / password auth), return `Ok(Auth::Accept)` instead of `Reject`, but set a new field `self.entity = None; self.fake_shell = true;` (do **not** populate `self.entity`/`AuthedEntity`, since there's no real entity). Then:

- `auth_succeeded` — currently early-returns effectively via `let Some(ref authed) = self.entity else { return Ok(()); }`. Add a branch: if `self.fake_shell`, still call `session.handle()` and spawn the fake-shell serving task (garbage prompt + echo-and-hang), without touching `session_registry`/broadcast (those are for real entities only).
- `channel_open_session` — currently `let Some(ref authed) = self.entity else { return Ok(false); };`. Change the guard to:
  ```rust
  if self.entity.is_none() && !self.fake_shell { return Ok(false); }
  ```
  and branch: real entity → existing welcome/chat behavior; `fake_shell` → serve a bogus `$ ` prompt, echo back typed bytes with a delay, never actually execute anything, hang until the client disconnects (reuse the existing `tokio::select! { channel.wait() ... interval.tick() ... }` pattern already in the codebase at ~lines 442-464 as the template, replacing the "Derpy ping" text with fake shell echoing).
- Update `ConnectionLog` write for this attempt: `fail_reason = None` is wrong (it wasn't a success either) — resolve by logging it as `fail_reason = Some("tarpitted - gave up")` **once the connection eventually drops** in `Drop for T2tHandler` (not at the accept-time `Auth::Accept`, since fail2ban's own "Accepted" semantics shouldn't fire) — i.e. the *initial* `ConnectionLog::create` for a fake-shell tarpit hit uses `fail_reason = Some("ip banned")`/`"user banned"` (whichever triggered it) and `tarpit_method = Some("fake_shell")`; it's never flipped to a success row, since `success`/`success_reason` are only ever set from the one true `log_auth_success` call path, which fake-shell never invokes.

### 3.6 Precise counting rules in `auth_publickey`/`auth_password` (Ambiguity #2/#3, concretely)

| Existing branch (lib.rs, ~line) | Increments peer_ip fail_count? | Increments user_id fail_count? | New `fail_reason` value |
|---|---|---|---|
| `auth_none` (234-245) | **No** — never call `log_auth_failure` here at all; it's a protocol probe every legitimate client sends | No | (no log row written) |
| `auth_password` (247-260) | Yes | No (no user_id resolvable) | `password auth not supported` |
| `auth_keyboard_interactive` (262-279) | Yes (add a `log_auth_failure` call here — currently missing) | No | `unsupported auth method` |
| unknown key (314) | Yes | No | `unknown key` |
| db error during key lookup (319) | **No** (infra error, not attacker signal) | No | (no log row — matches existing behavior, no `log_auth_failure` call today) |
| key expired / deleted_at (329) | Yes | **Yes** (ssh_key resolved → entity_id known → user_id known) | `key expired` |
| key expired valid_until (337) | Yes | Yes | `key expired (valid_until)` |
| entity not found (347) | Yes | No (entity_id known but entity row gone; no reliable user_id) | `entity not found` |
| entity db error (352) | No | No | (no log row) |
| entity expired (362) | Yes | Yes | `entity expired` |
| IP whitelist blocked (371-377) | Yes | Yes | `ip blocked by whitelist` |
| success (382-391) | **Clears** peer_ip AND user_id entries' `fail_count`/`window_start` (not `banned_until` if from an admin `ban_rules` row — those stay enforced; only the *auto-threshold* counter resets — see Ambiguity #1) | same | n/a — `success_reason = correct login` |

`log_auth_failure` gains a `user_id: Option<Uuid>` parameter and, internally, calls a new `TarpitState`-mutating helper (`record_failure(&tarpit, peer_ip, user_id, &thresholds)`) that increments counters, checks the window, and — if crossed — sets `banned_until` + bumps `trigger_count` (driving the next round-robin pick) + evaluates `banner_drip_eligible` via `ConnectionLog::peer_ip_has_known_good_history` (cached in the entry after first computation, invalidated to `false`→ never re-set to `true` mid-process except by an actual new success, per Ambiguity #1).

### 3.7 fail2ban log line for tarpit events

New line format appended alongside the existing "Failed publickey..." format, distinguishing tarpit triggers from ordinary auth rejections:

```
<timestamp> t2t sshd[0]: Failed publickey for invalid user tarpit from <peer_ip> port 0 ssh2
```

Reuse the *exact same* "Failed publickey for invalid user ... from <HOST> port ..." shape (just with a fixed sentinel username `tarpit`) so the **existing** fail2ban filter regex keeps matching it with zero changes — this is the simplest, most robust choice (see Ambiguity #6) rather than inventing a new line format requiring a filter regex update.

---

## 4. `tunnel2tunnel-web` changes

### 4.1 `crates/tunnel2tunnel-web/src/routes/connection_logs.rs` (new file)

```rust
#[derive(Deserialize)]
pub struct LogSearchQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub success: Option<bool>,
    pub method: Option<String>,   // tarpit_method
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct LogSearchResponse { pub items: Vec<ConnLogResponse>, pub total: i64, pub page: i64, pub page_size: i64 }

pub async fn search_connection_logs(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Query(q): Query<LogSearchQuery>,
) -> Result<Json<LogSearchResponse>, WebError> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let (rows, total) = ConnectionLog::search(&state.db, q.peer_ip.as_deref(), q.user_id,
        q.success, q.method.as_deref(), q.q.as_deref(), page, page_size).await.map_err(WebError::Core)?;
    Ok(Json(LogSearchResponse { items: rows.into_iter().map(ConnLogResponse::from).collect(), total, page, page_size }))
}
```

`ConnLogResponse` gains `user_id`, `attempted_password`, `fail_reason`/`success_reason`/`success`, `tarpit_method` fields, mirroring the model.

Register: `GET /api/admin/connection-logs` in `crates/tunnel2tunnel-web/src/lib.rs`, near line 101-102 (existing `/api/entities/{id}/logs` route stays untouched for the per-entity view).

### 4.2 `crates/tunnel2tunnel-web/src/routes/ban_rules.rs` (new file) — admin CRUD

```rust
GET    /api/admin/ban-rules            -> list_ban_rules (AdminUser)
POST   /api/admin/ban-rules            -> create_ban_rule (AdminUser; validates scope_type in ["peer_ip","user"])
DELETE /api/admin/ban-rules/{id}       -> delete_ban_rule (AdminUser)
```
`create_ban_rule` body: `{ scope_type, peer_ip?, user_id?, reason?, active_until? }` — runtime-validated against `["peer_ip","user"]` exactly like `access.rs:76-95` validates `subject_type`. `created_by = admin.0.id`.

### 4.3 `crates/tunnel2tunnel-web/src/routes/settings.rs` — admin threshold settings

```rust
GET  /api/admin/tarpit-settings   -> { threshold_count, threshold_window_seconds, enabled }
PUT  /api/admin/tarpit-settings   -> validates positive ints, calls Settings::set for each key
```

### 4.4 `routes/mod.rs` — add `pub mod connection_logs; pub mod ban_rules;`

---

## 5. Frontend changes

### 5.1 `frontend/src/api/admin.ts`

- Extend `ConnLog` interface with `user_id`, `attempted_password`, `fail_reason`, `success_reason`, `success`, `tarpit_method` (all matching the new DTO).
- Add `TarpitMethod = 'banner_drip' | 'slow_auth' | 'fake_shell'` type.
- Add `adminApi.searchConnectionLogs(params)`, `adminApi.listBanRules()`, `adminApi.createBanRule(...)`, `adminApi.deleteBanRule(id)`, `adminApi.getTarpitSettings()`, `adminApi.updateTarpitSettings(...)`.

### 5.2 `frontend/src/labels.ts` additions

```ts
export const tarpitMethodLabel: Record<TarpitMethod, string> = {
  banner_drip: 'Banner drip (endlessh-style)',
  slow_auth:   'Slow auth drip-feed',
  fake_shell:  'Fake interactive shell',
}
export const tarpitMethodOptions = Object.entries(tarpitMethodLabel).map(([value, label]) => ({ value, label }))

export const banScopeTypeLabel: Record<'peer_ip' | 'user', string> = { peer_ip: 'IP address', user: 'User account' }
export const banScopeTypeOptions = Object.entries(banScopeTypeLabel).map(([value, label]) => ({ value, label }))

export const failReasonLabel: Record<string, string> = { /* human labels for each fail_reason enum value */ }
export const successReasonLabel: Record<string, string> = { 'correct login': 'Correct login', 'admin reset ban': 'Admin reset ban' }
```

### 5.3 New page `frontend/src/pages/AdminConnectionLogsPage.vue`

Follows the `EntityDetailPage.vue` "Connection Log" lazy-load-on-demand pattern (293-309/645-678) but as its own page with:
- Filter bar (peer_ip text input, user select/free text, success dropdown fed by a 2-option array, method dropdown fed by `tarpitMethodOptions`, free-text search box) bound to reactive `ref`s, triggering `adminApi.searchConnectionLogs` on change (debounced) and on pagination click.
- `data-table` listing rows (reuse `.data-table` CSS class from `EntityDetailPage.vue`), rendering `fail_reason`/`success_reason` through `failReasonLabel`/`successReasonLabel`, `tarpit_method` through `tarpitMethodLabel`.
- Pager: "Page N of M" + Prev/Next buttons driven by `total`/`page_size` from the response — simplest possible UI, no fancy page-number list.

### 5.4 New page `frontend/src/pages/AdminBanRulesPage.vue`

Directly modeled on `EntityDetailPage.vue`'s Access Rules CRUD block (171-277/514-617):
- Toggleable "Add rule" form: `<select v-model="newRule.scope_type">` fed by `banScopeTypeOptions`, conditional sub-field (`peer_ip` text input vs a user-search/select), optional `active_until` datetime-local input (blank = indefinite, mirroring `EntityDetailPage`'s entity `valid_until` field pattern), `reason` free-text field.
- `data-table` of existing rules with a `×` delete button per row (`handleDeleteBanRule`).
- `blankBanRule()` factory + `handleAddBanRule`/`handleDeleteBanRule`, lazy-loaded via a boolean gate, exactly like `loadAccess()`/`handleAddAccess()`.
- A small "Global Thresholds" card at the top of the same page (or `SettingsPage.vue`) with two numeric inputs bound to `adminApi.getTarpitSettings()`/`updateTarpitSettings()`, styled like the existing `.field`/`.card` blocks in `SettingsPage.vue`.

### 5.5 `frontend/src/router/index.ts` — add routes

```ts
{ path: '/admin/connection-logs', name: 'admin-connection-logs', component: AdminConnectionLogsPage, meta: { requiresAuth: true } },
{ path: '/admin/ban-rules',       name: 'admin-ban-rules',       component: AdminBanRulesPage,       meta: { requiresAuth: true } },
```
Both pages should self-guard on `auth.user?.is_admin` (redirect or hide, following whatever pattern `AdminUsersPage.vue` uses — check it) since the API 403s for non-admins anyway but the UI shouldn't dead-end.

### 5.6 `frontend/src/components/AppShell.vue` nav — add, gated by `v-if="auth.user?.is_admin"` next to the existing `Admin: Users` link (~line 28):
```html
<li v-if="auth.user?.is_admin"><RouterLink to="/admin/connection-logs">Admin: Connection Logs</RouterLink></li>
<li v-if="auth.user?.is_admin"><RouterLink to="/admin/ban-rules">Admin: Ban Rules</RouterLink></li>
```

---

## 6. Test plan (per component, unit vs e2e, explicit legit-sequence test)

| Component | Test type | Location | What it asserts |
|---|---|---|---|
| Enum/CHECK constraint round-trip | Unit (needs DB) or a `#[sqlx::test]` | `crates/tunnel2tunnel-core/src/models/connection_log.rs` `mod tests` | INSERT with both reasons set → constraint violation; INSERT with neither set → violation; INSERT with only one set → OK, `success` generated column matches |
| `ban_rule.rs` CRUD + scope CHECK | Unit/`#[sqlx::test]` | `crates/tunnel2tunnel-core/src/models/ban_rule.rs` | peer_ip rule with user_id set → CHECK violation and vice versa; `list_active` excludes expired rows |
| Threshold counting query | Unit/`#[sqlx::test]` | `connection_log.rs` tests | `count_recent_failures_for_peer_ip` respects the time window and excludes rows outside it; `count_recent_failures_for_user` only counts rows where `user_id` is set |
| `ip_whitelist`-style pure logic: round-robin method selection | **Fast unit test, no network, no DB** | `crates/tunnel2tunnel-ssh/src/lib.rs` `mod tests` (extract `TarpitMethod::round_robin` as a free function so it's testable without spinning up the server) | `round_robin(0)==BannerDrip`, `round_robin(1)==SlowAuth`, `round_robin(2)==FakeShell`, `round_robin(3)==BannerDrip` (wraps) |
| Threshold-crossing state machine (`record_failure`) | **Fast unit test** operating directly on a bare `HashMap<String, TarpitEntry>` (no `PgPool`, no network) — isolate this as a pure function `fn record_failure(map: &mut HashMap<...>, key: &str, thresholds: &Thresholds, now: Instant) -> bool` (returns whether threshold was newly crossed) | `crates/tunnel2tunnel-ssh/src/lib.rs` `mod tests` | N-1 failures inside window → not banned; Nth failure inside window → banned, `trigger_count` incremented; failures outside the window don't accumulate (window resets) |
| "Success clears auto-threshold counter but not admin ban_rules ban" | **Fast unit test**, same pure state-machine function | same | after ban triggered by threshold, a simulated success resets `fail_count`/`window_start` to zero but leaves an admin-set `banned_until` (simulated as pre-seeded, non-auto-cleared) intact |
| `banner_drip_eligible` flips to false once, never back | Unit test on the pure struct, with a stub async fn replacing the DB call | same | once `banner_drip_eligible=false` is set, subsequent `record_failure` calls don't re-enable it |
| Banner-drip line generator | **Fast unit test** | `crates/tunnel2tunnel-ssh/src/lib.rs` `mod tests` | generated line never starts with `"SSH-"` (RFC4253 requirement), always CRLF-terminated, length within a sane bound |
| fail2ban line format (both existing + new tarpit sentinel line) | **Fast unit test** | `crates/tunnel2tunnel-ssh/src/lib.rs` `mod tests` | exact string shape matches what `contrib/fail2ban/filter.d/tunnel2tunnel.conf`'s regex expects — assert with the *same* regex crate against the generated string, so a future accidental format drift fails this test before it ever reaches production log parsing |
| DTO validation for ban-rule scope_type / connection-log query params | **Fast unit test**, no DB | `crates/tunnel2tunnel-web/src/routes/ban_rules.rs` (or a small `#[cfg(test)]` block calling the validation function directly) | invalid `scope_type` rejected; `page_size` clamps to `[1,200]` |
| Admin API integration (ban-rules CRUD, connection-log search, tarpit-settings) | **Axum integration test** (`tower::ServiceExt::oneshot`, existing style if this repo has any — otherwise a small new test module) using a real Postgres test DB (same as the e2e harness's DB bootstrap) | `crates/tunnel2tunnel-web/tests/ban_rules.rs` (new) | full round trip: create rule via API → appears in list → delete → gone; non-admin gets 403 |
| **Legit-login-sequence-must-not-be-tarpitted** (explicit, required) | **e2e**, reuses `crates/t2t/tests/tunnel_e2e.rs` fixtures (real OpenSSH subprocess, DB bootstrap, `generate_test_keypair`, `free_port`, server-start-and-poll helpers) | `crates/t2t/tests/tarpit_e2e.rs` (new) | 1. Seed a `ban_rules` row (or drive enough manufactured prior failures via direct `ConnectionLog::create` calls) so the test peer_ip/user is definitely over-threshold and `banned_until` is set. 2. Register a real entity + valid keypair for that same identity, exactly reproducing the reference log sequence: real OpenSSH client connects, does the `none` probe, offers the real key, signs, succeeds. 3. Assert the OpenSSH subprocess actually completes login (no hang, exits 0 / prints expected banner) within a short timeout — proving neither banner-drip (would hang forever pre-ID) nor slow-auth (would just add seconds, still bearable, but assert it's fast) nor fake-shell intercepted it. 4. Assert the resulting `connection_logs` row for this attempt has `success=true`, `success_reason='correct login'`, `tarpit_method IS NULL`. |
| `auth_none` never increments any counter / never logged | **e2e or a focused unit test** stubbing the state machine — assert that after N `auth_none` probes with zero real credential attempts, threshold is never crossed | `crates/t2t/tests/tarpit_e2e.rs` or ssh crate unit test | connect N+5 times doing only the initial probe (which real OpenSSH always does) and abort before offering a key each time → never banned |
| Banner-drip end-to-end behavior | **e2e**, raw `tokio::net::TcpStream` (not OpenSSH — OpenSSH would just hang, which is the point, so a raw client is needed to assert the drip content itself) | `crates/t2t/tests/tarpit_e2e.rs` | after crossing threshold with round-robin landing on `BannerDrip`, a raw TCP connect to the SSH port receives repeated non-`"SSH-"`-prefixed lines and never receives the real `SSH-2.0-...` id within a bounded wait |
| Slow-auth-drip / fake-shell end-to-end behavior | **e2e**, raw `russh`-free minimal client using `tokio::io` reading raw bytes post-KEX is impractical without a full SSH client — instead, drive these through the **existing OpenSSH-subprocess harness** but with a deliberately *unknown* key (never registered) so real auth legitimately fails, and assert (a) wall-clock time elapsed for slow-auth is noticeably higher than baseline, (b) for fake-shell, the OpenSSH client's stdout contains the bogus prompt text and the process must be killed by the test's timeout/`ChildGuard` rather than exiting cleanly | `crates/t2t/tests/tarpit_e2e.rs` |

Rationale for the unit/e2e split: the threshold engine, round-robin selection, and format-generation are pure/deterministic logic — extracting them as free functions decoupled from `PgPool`/`TcpStream` (as noted inline above) lets the bulk of the interesting logic be covered by fast, deterministic unit tests, matching the existing `ip_whitelist.rs`/`pubkey.rs` precedent — reserving the heavyweight OpenSSH-subprocess e2e harness only for the handful of tests that genuinely require observing real wire-level SSH/TCP behavior (which is unavoidable for banner-drip and the legit-sequence guarantee).

---

## 7. Files to create / modify

**Create:**
- `migrations/008_tarpit.sql`
- `crates/tunnel2tunnel-core/src/models/ban_rule.rs`
- `crates/tunnel2tunnel-web/src/routes/connection_logs.rs`
- `crates/tunnel2tunnel-web/src/routes/ban_rules.rs`
- `crates/t2t/tests/tarpit_e2e.rs`
- `frontend/src/pages/AdminConnectionLogsPage.vue`
- `frontend/src/pages/AdminBanRulesPage.vue`

**Modify:**
- `crates/tunnel2tunnel-core/src/models/connection_log.rs` — struct fields, `create()` signature, new `count_recent_failures_for_peer_ip`/`for_user`, `peer_ip_has_known_good_history`, `search`
- `crates/tunnel2tunnel-core/src/models/mod.rs` — add `pub mod ban_rule;`
- `crates/tunnel2tunnel-ssh/src/lib.rs` — near `T2tServer`/`T2tHandler` structs (111-161): add `tarpit: TarpitState` field; near `start()` (80-107): replace `run_on_address` with custom accept loop + spawn settings-refresher; near `log_auth_failure`/`log_auth_success` (164-228): add `user_id`, `attempted_password`, `tarpit_method` params and threshold-recording/clearing calls; near `auth_none` (234-245): explicitly document/confirm no counter increment; near `auth_password` (247-260): log attempted password, slow-auth delay hook; near `auth_keyboard_interactive` (262-279): add missing `log_auth_failure` call; near `auth_publickey` (298-394): per-branch counter/user_id wiring per §3.6 table, slow-auth delay + fake-shell accept substitution; near `auth_succeeded` (397-482) and `channel_open_session` (485-526): fake-shell branch; near `chrono_like_timestamp`/`append_to_file` (917-940): add banner-drip line generator + tarpit fail2ban line helper
- `crates/tunnel2tunnel-web/src/routes/mod.rs` — add `pub mod connection_logs; pub mod ban_rules;`
- `crates/tunnel2tunnel-web/src/routes/entities.rs` — `ConnLogResponse` (~535-556) gains new fields (kept for backward compat on the per-entity route)
- `crates/tunnel2tunnel-web/src/routes/settings.rs` — add `get_tarpit_settings`/`update_tarpit_settings`
- `crates/tunnel2tunnel-web/src/lib.rs` — register `/api/admin/connection-logs`, `/api/admin/ban-rules`, `/api/admin/ban-rules/{id}`, `/api/admin/tarpit-settings` near the existing route block (~101-102, ~106-108)
- `contrib/fail2ban/filter.d/tunnel2tunnel.conf` — **no change needed** if the "reuse the exact existing line shape with sentinel username `tarpit`" resolution (§3.7 / Ambiguity #6) is adopted; note this explicitly in the PR/commit so reviewers don't go looking for a missing filter update
- `frontend/src/api/admin.ts` — extended `ConnLog`, new API functions
- `frontend/src/labels.ts` — new label/option exports
- `frontend/src/router/index.ts` — two new routes
- `frontend/src/components/AppShell.vue` — two new nav links
- `CLAUDE.md` — add `008_tarpit.sql` row to the migrations table, mention new env-var-free admin-configurable thresholds (via settings table, not env vars)

---

## 8. Design ambiguities — explicit resolutions

**#1 — "Success clears ban state for future connections" is explicitly marked "not required" in the spec, yet contradicts pure peer_ip banner-drip (a legit device on a banned/shared IP could never connect at all under pure IP-blocking, since banner-drip intercepts before the SSH ID is ever sent).**
Resolution: banner-drip is made **permanently ineligible** for any `peer_ip` that has *ever* produced one genuine `success_reason='correct login'` row in `connection_logs` (tracked via `banner_drip_eligible` in the in-memory cache, computed once via `ConnectionLog::peer_ip_has_known_good_history` and never re-enabled). Methods b/c (slow-auth, fake-shell) remain available for such IPs since both still let the real SSH protocol run to completion for a *genuinely valid* key — the tarpit logic in both only ever intervenes on branches that were already going to `Reject`, so a legitimate key's path to `Auth::Accept` is never touched. Per the literal "not required" wording, we do **not** clear the auto-threshold ban counter on success for *future* connections from the same identity (an attacker sharing a NAT/VPN IP with a legit device shouldn't get to un-ban that IP by any other means); we only guarantee the currently-succeeding session itself is never interrupted (trivially true, since success always short-circuits before any tarpit branch is reached).

**#2 — Does the `none`-probe ever get filtered out of a counter, or never counted in the first place?**
Resolution: `auth_none` never calls `log_auth_failure`/writes a `connection_logs` row at all (there is nothing to filter — no log row exists for this branch). This is deliberate: every legitimate SSH client sends this probe, so counting or even logging it would produce noise and, worse, could trip the threshold purely from legitimate traffic patterns.

**#3 — What does "ban by user" mean, given SSH auth is by pubkey, not username?**
Resolution: "user" = the owning `users.id` (the human/account row), reached only via `SshKey → Entity.user_id`, exactly mirroring how `entity_access.subject_user_id`/`all_user_entities` already scope by user account rather than by individual entity. This is never resolvable for pure scanner noise (unknown key), so only failure branches that got as far as resolving a real `Entity` (key expired, key expired via valid_until, entity expired, IP whitelist blocked) count toward the per-user threshold — see the table in §3.6. Banning "by user" therefore models banning a known, previously-legitimate account whose credentials are now being misused or are stale/expired but still repeatedly attempted (not anonymous brute-forcers, who can never resolve to a user_id).

**#4 — `GENERATED ALWAYS AS ... STORED` vs app-computed `success`.**
Resolution: use a Postgres `GENERATED ALWAYS AS (success_reason IS NOT NULL) STORED` column. Justification: the CHECK constraint already guarantees exactly one reason column is set at the DB layer, so deriving `success` at the same layer keeps a single source of truth immune to application-level drift (a future code path that forgets to compute it correctly, or a raw `UPDATE` from a migration/ops script). `sqlx::FromRow` needs no special handling — a `GENERATED ... STORED` column reads back through `SELECT *` exactly like any other column, so `pub success: bool` on the struct works unchanged; the only constraint is that `INSERT`/`UPDATE` statements must never attempt to write to it directly (already true, since `ConnectionLog::create` only inserts `fail_reason`/`success_reason`).

**#5 — Revive the existing `settings` key/value table for thresholds, or add dedicated columns/rows?**
Resolution: reuse `settings` (already seeded with `fail2ban_log_path`/`signup_enabled`, already has `Settings::get`/`set` helpers) rather than inventing a new mechanism — it's precisely a generic admin-configurable-value store and the three new keys (`tarpit_threshold_count`, `tarpit_threshold_window_seconds`, `tarpit_enabled`) fit its existing shape with zero schema changes needed beyond the seed `INSERT`s in the new migration.

**#6 — Should tarpit-triggering events feed the *external* fail2ban process, or are the internal ban-rule engine and fail2ban meant to be independent, parallel defenses?**
Resolution: **both, via the same existing line format.** Rather than inventing a new fail2ban line shape (which would require an accompanying filter-regex change and carries risk of the existing filter silently stopping working if not updated everywhere it's deployed), tarpit-triggering rejections write the *same* `"Failed publickey for invalid user ... from <peer_ip> port 0 ssh2"` line shape used today, using a fixed sentinel username `tarpit` in place of a real fingerprint. This means: (a) zero changes needed to `contrib/fail2ban/filter.d/tunnel2tunnel.conf`, (b) an external fail2ban deployment automatically also starts banning at the OS/firewall level for IPs that trip our internal tarpit threshold (since those failures already accumulate through the identical log line format fail2ban already scans), giving genuine defense-in-depth (our tarpit wastes the scanner's time/resources *now*; fail2ban firewalls the IP out entirely a bit later) without the two systems needing to know about each other.

**#7 — Where does the pre-auth (`banner_drip`) decision live given `russh`'s `run_stream` can't be intercepted mid-flow?**
Resolution (elaborated in §0/§3.2): replace `T2tServer::run_on_address(...)` with a hand-rolled `TcpListener::accept()` loop in `start()`, calling `russh::server::run_stream` directly only for connections not selected for banner-drip; connections selected for banner-drip never touch russh and are served by a raw loop owning the `TcpStream`. This is the only way to honor "never even send the real SSH-2.0 identification string" given `russh` 0.61's public API.

### Critical Files for Implementation
- /home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs
- /home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/connection_log.rs
- /home/user/git/luckydonald/tunnel2tunnel/migrations/008_tarpit.sql (new)
- /home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/lib.rs
- /home/user/git/luckydonald/tunnel2tunnel/crates/t2t/tests/tunnel_e2e.rs (fixtures reused by new `tarpit_e2e.rs`)