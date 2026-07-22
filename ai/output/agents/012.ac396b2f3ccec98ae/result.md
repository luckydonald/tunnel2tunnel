All findings gathered. Here is the full report.

## File map

- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs` — the entire SSH crate (940 lines, single file).
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/ip_whitelist.rs` — IP whitelist evaluator.
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/entity.rs`, `ssh_key.rs`, `entity_access.rs`, `connection_log.rs` — DB models used by the SSH handler.
- `/home/user/git/luckydonald/tunnel2tunnel/crates/t2t/src/main.rs` — wires `FAIL2BAN_LOG_PATH` env var into `SshConfig`.
- `/home/user/git/luckydonald/tunnel2tunnel/contrib/fail2ban/filter.d/tunnel2tunnel.conf` — fail2ban regex filter matching the log format.
- `/home/user/git/luckydonald/tunnel2tunnel/CLAUDE.md` — architecture notes, incl. the russh Handle/tokio::spawn gotcha.

---

## 1. russh `Handler` implementation

All in `crates/tunnel2tunnel-ssh/src/lib.rs`, `impl Handler for T2tHandler` block starting at line 231.

- `auth_none` — line 234-245
- `auth_password` — line 247-260 (always rejects; password auth is not supported, just logs+rejects)
- `auth_keyboard_interactive` — line 262-279 (also just rejects)
- `auth_publickey_offered` — line 281-296 (accepts all probes; real check happens later)
- `auth_publickey` — line 298-394 (the real auth logic: key lookup → expiry checks → entity lookup → entity expiry → IP whitelist → accept)
- `auth_succeeded` — line 397-482 (post-auth: registers session, opens push-message channel, broadcasts arrival)
- `channel_open_session` — line 485-526 (client-initiated interactive channel, treated as chat)
- `tcpip_forward` — line 529-554 (`-R` port registration into `server_slots`)
- `cancel_tcpip_forward` — line 556-579
- `channel_open_direct_tcpip` — line 582-712 (`-L` forwarding: resolves target entity, checks `EntityAccess::check_access`, splices channels)
- `data`, `channel_eof`, `channel_close` — line 714-774 (bridge/chat relaying)

### Per-session/connection state tracking

- `T2tServer` (server factory, one per listener) — line 111-116: `pool`, `server_slots` (`Arc<Mutex<HashMap<(Uuid,u32),(Handle,String)>>>`), `session_registry` (`Arc<Mutex<HashMap<Uuid, SessionEntry>>>`), `fail2ban: Option<Arc<String>>`.
- `new_client` (line 121-138) creates one `T2tHandler` per TCP connection, generating `conn_id = Uuid::now_v7()` and capturing `peer_ip`.
- `T2tHandler` (per-connection struct) — line 151-161:
```rust
struct T2tHandler {
    pool: PgPool,
    server_slots: ServerSlots,
    session_registry: SessionRegistry,
    fail2ban: Option<Arc<String>>,
    conn_id: Uuid,
    peer_ip: String,
    entity: Option<AuthedEntity>,
    bridges: HashMap<ChannelId, (Handle, ChannelId)>,
    log_id: Option<Uuid>,
}
```
- `AuthedEntity` (line 143-147): `{ entity: Entity, user_id: Uuid }` — set once auth succeeds (line 391).
- `SessionEntry` (line 44-48): `{ handle: Handle, session_channel_id: Option<ChannelId> }`, keyed by `conn_id` in the global `SessionRegistry`.
- `bridges: HashMap<ChannelId, (Handle, ChannelId)>` maps a client-local channel to a paired (server Handle, server channel) for direct-tcpip splicing — populated in `channel_open_direct_tcpip` (line 694-695), consulted in `data`/`channel_eof`/`channel_close`.
- Cleanup happens in `impl Drop for T2tHandler` (line 776-826): logs disconnect, removes from `session_registry`, purges `server_slots` entries owned by that entity, and marks `ConnectionLog.set_ended`.

There is no per-IP or per-attempt counter/state anywhere — nothing tracks repeated failures across connections in-process (each `T2tHandler` is fresh per TCP connection and discarded on drop).

---

## 2. Auth-attempt logging (`tracing::info!` etc.) — exact locations and fields

| Location | Level | Fields | Message |
|---|---|---|---|
| line 126 | info | `%peer_ip, %conn_id` | `"SSH: new connection"` (in `new_client`) |
| line 235-240 | info | `peer_ip = %self.peer_ip, %user, available = "publickey"` | `"SSH: auth attempt (none) — rejected"` |
| line 248-253 | info | `peer_ip, %user, password_len = password.len(), available = "publickey"` | `"SSH: auth attempt (password) — rejected (password auth not supported)"` |
| line 268-273 | info | `peer_ip, %user, %submethods, available = "publickey"` | `"SSH: auth attempt (keyboard-interactive) — rejected"` |
| line 287-292 | info | `peer_ip, %user, %fp, key_algo = key.algorithm().as_str()` | `"SSH: publickey offered (probe, no signature yet)"` (in `auth_publickey_offered`) |
| line 308 | info | `peer_ip = %self.peer_ip, %user, %fp` | `"SSH: auth attempt (publickey)"` |
| line 314 | info | `%fp, peer_ip` | `"SSH: auth rejected — unknown key"` |
| line 319 | error | `err = %e, %fp, peer_ip` | `"SSH: auth error — db error during key lookup"` |
| line 324 | debug | `%fp, key_id = %ssh_key.id, entity_id = %ssh_key.entity_id` | `"SSH: key found in db"` |
| line 329 | info | `%fp, key_id = %ssh_key.id` | `"SSH: auth rejected — key expired (deleted_at)"` |
| line 337 | info | `%fp, key_id = %ssh_key.id` | `"SSH: auth rejected — key expired (valid_until)"` |
| line 347 | info | `%fp, entity_id = %ssh_key.entity_id` | `"SSH: auth rejected — entity not found"` |
| line 352 | error | `err = %e, %fp` | `"SSH: auth error — db error loading entity"` |
| line 357 | debug | `%fp, entity_id = %entity.id, entity_name` | `"SSH: entity loaded"` |
| line 362 | info | `entity_id = %entity.id, entity_name` | `"SSH: auth rejected — entity expired"` |
| line 371-376 | info | `entity_id, entity_name, peer_ip` | `"SSH: auth rejected — IP blocked by whitelist"` |
| line 382-388 | info | `entity_id = %entity.id, entity_name, peer_ip = %self.peer_ip, %fp` | `"SSH: auth accepted"` |
| line 540 | info | `%entity_id, proxy_port = port` | `"server registered port"` |
| line 592 | warn | `peer_ip = %self.peer_ip` | `"direct-tcpip: rejected — not authenticated"` |
| line 598-605 | info | `client_entity = %client_entity.id, client_entity_name, peer_ip, host = host_to_connect, port = port_to_connect` | `"direct-tcpip: channel open request"` |
| line 610-615 | info | `host, port, client_entity` | `"direct-tcpip: rejected — target hostname not found (not a UUID or known alias)"` |
| line 623-627 | info | `target_entity, client_entity` | `"direct-tcpip: rejected — target entity not found in db"` |
| line 648-655 | warn | `client_entity, target_entity, target_entity_name, host, port` | `"direct-tcpip: rejected — access denied"` |
| line 668-674 | info | `target_entity, target_entity_name, proxy_port` | `"direct-tcpip: rejected — target server has no registered port (server not connected?)"` |
| line 701-710 | info | `client_entity, target_entity, target_entity_name, host, port, client_ch, server_ch` | `"direct-tcpip: bridge established"` |
| line 779-785 | info | `entity_id, entity_name, peer_ip` (or just `peer_ip`) | `"SSH: connection closed (authenticated)"` / `"(unauthenticated)"` |

Notable field-name inconsistencies you'll want to normalize if extending this for a tarpit feature: sometimes `entity_id`/`entity_name` are plain field names, sometimes `client_entity`/`target_entity` (holding the `Uuid`, not `_id` suffixed). `fp` is the consistent name for the key fingerprint. `peer_ip` is consistent throughout. `conn_id` only appears in the `new_client` and `auth_succeeded`/`channel_open_session` debug logs, not in the auth-attempt logs themselves — i.e. the auth log lines currently have **no `conn_id` field**, only `peer_ip`.

---

## 3. fail2ban-format log writing

`SshConfig.fail2ban_log_path: Option<String>` (line 31) ← populated from `FAIL2BAN_LOG_PATH` env var in `/home/user/git/luckydonald/tunnel2tunnel/crates/t2t/src/main.rs:39` and passed through at line 82.

In `start()` (line 93): `let fail2ban = config.fail2ban_log_path.map(Arc::new);` stored on `T2tServer.fail2ban: Option<Arc<String>>` and cloned into every `T2tHandler.fail2ban` in `new_client` (line 131).

Actual writer logic lives in two `T2tHandler` methods:

```rust
// lib.rs:164-197
async fn log_auth_failure(
    &self,
    fingerprint: Option<&str>,
    reason: &str,
) {
    let now = time::OffsetDateTime::now_utc();
    if let Err(e) = ConnectionLog::create(
        &self.pool, None, Some(&self.peer_ip), fingerprint, false, Some(reason), None, now,
    ).await {
        tracing::warn!(err = %e, "failed to write auth failure log");
    }

    if let Some(path) = &self.fail2ban {
        let fp = fingerprint.unwrap_or("unknown");
        let line = format!(
            "{} t2t sshd[0]: Failed publickey for invalid user {} from {} port 0 ssh2\n",
            chrono_like_timestamp(),
            fp,
            self.peer_ip,
        );
        if let Err(e) = append_to_file(path, &line).await {
            tracing::warn!(err = %e, "fail2ban write failed");
        }
    }
}
```

```rust
// lib.rs:199-228
async fn log_auth_success(&mut self, entity: &Entity, fingerprint: &str) {
    let now = time::OffsetDateTime::now_utc();
    match ConnectionLog::create(
        &self.pool, Some(entity.id), Some(&self.peer_ip), Some(fingerprint), true, None, None, now,
    ).await {
        Ok(log) => self.log_id = Some(log.id),
        Err(e) => tracing::warn!(err = %e, "failed to write auth success log"),
    }

    if let Some(path) = &self.fail2ban {
        let line = format!(
            "{} t2t sshd[0]: Accepted publickey for {} from {} port 0 ssh2\n",
            chrono_like_timestamp(),
            entity.id,
            self.peer_ip,
        );
        if let Err(e) = append_to_file(path, &line).await {
            tracing::warn!(err = %e, "fail2ban write failed");
        }
    }
}
```

Both are called from `auth_publickey` (failure path calls at lines 315, 330, 338, 348, 363, 378; success at line 390) and `auth_password` (line 255 calls `log_auth_failure(None, "password auth not supported")`).

Helper functions used, both at the bottom of the file:

```rust
// lib.rs:917-929
fn chrono_like_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    let month = months[now.month() as usize - 1];
    format!(
        "{} {:2} {:02}:{:02}:{:02}",
        month, now.day(), now.hour(), now.minute(), now.second(),
    )
}
```

```rust
// lib.rs:931-940
async fn append_to_file(path: &str, content: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    Ok(())
}
```

Matching fail2ban filter regex, `/home/user/git/luckydonald/tunnel2tunnel/contrib/fail2ban/filter.d/tunnel2tunnel.conf`:
```
[Definition]
# Matches lines written by tunnel2tunnel to FAIL2BAN_LOG_PATH
# Format mirrors sshd: "... Failed publickey for invalid user <fp> from <IP> port <p> ssh2"
failregex = ^.+ t2t sshd\[\d+\]: Failed publickey for invalid user .+ from <HOST> port \d+ ssh2$
ignoreregex =
```
Note: only the "Failed" line is matched by the fail2ban filter (bans on failures); the "Accepted" line written on success is not matched by anything — it's just informational, present in the log file but not used by fail2ban's ban logic.

There is no dedicated fail2ban module — it's inlined directly into `T2tHandler`'s two logging methods plus the two free-standing helper functions above, all in `lib.rs`.

---

## 4. Entity lookup / key fingerprint lookup / entity_access checks

All in `tunnel2tunnel-core` crate, imported at `lib.rs:19-27`:
```rust
use tunnel2tunnel_core::{
    ip_whitelist,
    models::{
        connection_log::ConnectionLog,
        entity::Entity,
        entity_access::EntityAccess,
        ssh_key::SshKey,
    },
};
```

- **Fingerprint → key**: `SshKey::find_by_fingerprint(pool, fingerprint)` — `crates/tunnel2tunnel-core/src/models/ssh_key.rs:25-36`. Query: `SELECT * FROM ssh_keys WHERE fingerprint = $1 AND deleted_at IS NULL`. Called from `auth_publickey` at `lib.rs:311`.
- **Entity by id**: `Entity::find_by_id_only(pool, id)` — `crates/tunnel2tunnel-core/src/models/entity.rs:43-54`. Query: `SELECT * FROM entities WHERE id = $1 AND deleted_at IS NULL`. Called at `lib.rs:344` (auth) and `lib.rs:620` (direct-tcpip target lookup).
- **Access check**: `EntityAccess::check_access(pool, target_entity_id, client_entity_id, client_user_id, target_user_id)` — `crates/tunnel2tunnel-core/src/models/entity_access.rs:96-123`. SQL checks `subject_type` against `'public_lite'`, `'all_mine'`, `'all_user_entities'`, or `'entity'` match. Called at `lib.rs:637-645`.
- **Hostname alias → entity**: `EntityAccess::find_entity_by_hostname(pool, hostname)` — `entity_access.rs:141-153`, used inside `resolve_target_entity` (`lib.rs:830-839`), which first tries parsing the hostname as a `Uuid` directly, else falls back to this alias lookup.

`Entity` struct (`entity.rs:8-22`) fields: `id, user_id, entity_type (renamed "type"), name, description, ip_whitelist: Option<String>, valid_until: Option<OffsetDateTime>, ts: TimestampsSoftDelete`.

`SshKey` struct (`ssh_key.rs:8-22`) fields: `id, entity_id, algorithm, key_data, comment, fingerprint, name, valid_until: Option<OffsetDateTime>, ts: TimestampsSoftDelete`. Note `ts.soft_delete.deleted_at` is used at `lib.rs:327` as a secondary "expiry" check distinct from `valid_until` — both are checked separately (lines 327-333 and 335-341).

---

## 5. IP whitelist evaluation / existing rate-limiting or banning

`crates/tunnel2tunnel-core/src/ip_whitelist.rs` — single function `pub fn evaluate(rules: &str, peer_ip: &str) -> bool` (line 14-38). It's a **pure, stateless** function: parses newline-separated rules (CIDR / glob / regex `s/…/ ` / plain IP, each optionally `!`-prefixed for deny), first-match-wins, empty ruleset = allow-all, non-empty with no match = deny. No state, no counters, no persistence — called fresh on every `auth_publickey` at `lib.rs:370`:
```rust
if let Some(ref whitelist) = entity.ip_whitelist {
    if !ip_whitelist::evaluate(whitelist, &self.peer_ip) {
        ...
        self.log_auth_failure(Some(&fp), "IP blocked by whitelist").await;
        return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
    }
}
```

**There is no rate-limiting or banning mechanism anywhere in the codebase** — not in-process, not DB-backed. The only "ban" surface is the external fail2ban integration via the log file described in §3 (an out-of-process tool watching the log and presumably `iptables`-banning at the OS/network level). No attempt counters, no sliding windows, no per-IP or per-fingerprint tracking structures exist in `T2tHandler`, `T2tServer`, or `tunnel2tunnel-core`. `grep` across the crate confirms no other module references "rate" or "ban" logic.

---

## 6. Existing "shell"/`channel_open_session` handling for authenticated users, and `tokio::spawn` gotcha

There is no real shell — `channel_open_session` (line 485-526) just wires the client-opened channel into the chat/broadcast system (registers it in `session_registry`, sends a decorative "Welcome" banner with ANSI colors and emoji, then spawns a task that drains the channel doing nothing but watching for EOF/Close). This is a reasonable template to build a fake/tarpit shell from — it already shows the pattern of: get `channel.id()`, `session.handle()`, write bytes via `handle.data(ch_id, bytes).await`, then `tokio::spawn` a loop consuming `channel.wait()` to keep it alive.

The `auth_succeeded` server-initiated channel (line 397-482) demonstrates the actual gotcha the CLAUDE.md warns about:
```rust
// lib.rs:415-419 (comment) and 423-472 (code)
// Open a server-initiated session channel to the client for push messages, in the
// background. `channel_open_session()` awaits a confirmation that round-trips
// through this same connection's single-task event loop — awaiting it inline here
// (still inside that loop's packet dispatch) would deadlock permanently, since the
// loop can never get back around to servicing its own request.
let registry = self.session_registry.clone();
...
tokio::spawn(async move {
    match handle.channel_open_session().await {
        Ok(ch) => { ... }
        Err(e) => { ... }
    }
});
```
This is also called out in `CLAUDE.md` line 203:
> **russh `Handle` confirmation calls** (e.g. `channel_open_session()`): must be `tokio::spawn`ed, never `.await`ed inline from a `Handler` callback (e.g. `auth_succeeded`) — the confirmation round-trips through that same connection's single-task event loop, so awaiting it synchronously inside the callback deadlocks permanently.

The periodic-ping sub-task pattern (line 442-464, "Derpy Hooves" pings every 5 minutes via `tokio::time::interval` + `tokio::select!` racing `channel.wait()` against the timer) is a good existing template for building a tarpit's periodic-keepalive/stalling behavior.

Also note: for **unauthenticated** connections there is currently no `channel_open_session` handling at all beyond the `let Some(ref authed) = self.entity else { return Ok(false); };` guard at line 490 — any pre-auth or post-reject channel open attempt is flatly refused, so a tarpit "fake shell for bogus auth" would need new logic (either allowing certain auth failures through as `Auth::Accept` into a trap state, or handling channel-open even when `self.entity` is `None`, gated on some new tarpit flag).

---

## 7. Server / per-connection state struct definitions (extension points)

```rust
// lib.rs:111-116 — one instance per listener
struct T2tServer {
    pool: PgPool,
    server_slots: ServerSlots,           // Arc<Mutex<HashMap<(Uuid, u32), (Handle, String)>>>
    session_registry: SessionRegistry,   // Arc<Mutex<HashMap<Uuid, SessionEntry>>>
    fail2ban: Option<Arc<String>>,
}
```

```rust
// lib.rs:151-161 — one instance per TCP connection (created in new_client, line 121-138)
struct T2tHandler {
    pool: PgPool,
    server_slots: ServerSlots,
    session_registry: SessionRegistry,
    fail2ban: Option<Arc<String>>,
    conn_id: Uuid,
    peer_ip: String,
    entity: Option<AuthedEntity>,
    bridges: HashMap<ChannelId, (Handle, ChannelId)>,
    log_id: Option<Uuid>,
}
```

```rust
// lib.rs:143-147
#[derive(Clone)]
struct AuthedEntity {
    entity: Entity,
    user_id: Uuid,
}
```

```rust
// lib.rs:44-48
struct SessionEntry {
    handle: Handle,
    session_channel_id: Option<ChannelId>,
}
```

```rust
// lib.rs:29-36 — config passed into start()
pub struct SshConfig {
    pub ssh_port: u16,
    pub fail2ban_log_path: Option<String>,
    pub host_key_path: String,
    pub host_key_password: Option<String>,
}
```

For a tarpit feature, the natural extension points are:
- Add a field to `T2tServer`/`T2tHandler`, e.g. a shared `Arc<Mutex<HashMap<String /* peer_ip */, TarpitState>>>`, constructed in `start()` (line 95-100, alongside `server_slots`/`session_registry`) and cloned into each `T2tHandler` in `new_client` (line 127-138), the same pattern already used for `server_slots`/`session_registry`/`fail2ban`.
- Failure-counting/deciding-to-tarpit would hook into `log_auth_failure` (line 164-197) since that's the single choke point every rejection path in `auth_publickey`/`auth_password` already funnels through.
- A fake shell would extend `channel_open_session` (line 485-526), but that function currently early-returns `Ok(false)` for unauthenticated connections (line 490) — you'd need to either flip some sessions into `Auth::Accept` with an `entity: None` "tarpit" marker (perhaps a new enum variant instead of `Option<AuthedEntity>`, e.g. `enum SessionKind { Authed(AuthedEntity), Tarpit }`) or bypass that guard when a new tarpit flag is set on `T2tHandler`.
- `SshConfig` (line 29-36) is where you'd add new env-var-driven tarpit knobs (mirroring how `fail2ban_log_path` flows from `crates/t2t/src/main.rs:39` through `SshConfig` into `T2tServer`/`T2tHandler`).