## Root cause found

The hang is a **self-deadlock inside russh's single-task connection event loop**, triggered by the newly-added `auth_succeeded` handler (commit `7db0eb2`, "welcome message, Derpy ping, broadcast notifications, chat relay"). It fires on **every** successful auth (including plain `-N -R` tunnels), calls `session.handle().channel_open_session().await`, and that call can never resolve because the only task that could ever service it is the task currently blocked awaiting it. This exactly reproduces "SSH_MSG_USERAUTH_PK_OK sent fine, then the connection just hangs — no accept, no reject, nothing," because the already-encoded `USERAUTH_SUCCESS` packet is never flushed to the socket (flush happens only *after* the stuck call would return).

---

### 1. Full `auth_publickey` / `auth_publickey_offered` / `auth_succeeded` code

`/Users/user/Documents/programming/Rust/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs:284-477`

```rust
async fn auth_publickey_offered(
    &mut self,
    user: &str,
    key: &russh::keys::PublicKey,
) -> Result<Auth, Self::Error> {
    let fp = format!("{}", key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256));
    tracing::info!(peer_ip = %self.peer_ip, %user, %fp, key_algo = key.algorithm().as_str(),
        "SSH: publickey offered (probe, no signature yet)");
    // Accept all probes — actual key validation happens in auth_publickey
    Ok(Auth::Accept)
}

async fn auth_publickey(
    &mut self,
    user: &str,
    key: &russh::keys::PublicKey,
) -> Result<Auth, Self::Error> {
    let fp = format!("{}", key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256));
    tracing::info!(peer_ip = %self.peer_ip, %user, %fp, "SSH: auth attempt (publickey)");

    let ssh_key = match SshKey::find_by_fingerprint(&self.pool, &fp).await {
        Ok(Some(k)) => k,
        Ok(None) => { /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false }); }
        Err(e) => { /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false }); }
    };

    if let Some(valid_until) = ssh_key.ts.soft_delete.deleted_at {
        if valid_until < time::OffsetDateTime::now_utc() {
            /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
        }
    }
    if let Some(valid_until) = ssh_key.valid_until {
        if valid_until < time::OffsetDateTime::now_utc() {
            /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
        }
    }

    let entity = match Entity::find_by_id_only(&self.pool, ssh_key.entity_id).await {
        Ok(Some(e)) => e,
        Ok(None) => { /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false }); }
        Err(e) => { /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false }); }
    };

    if let Some(valid_until) = entity.valid_until {
        if valid_until < time::OffsetDateTime::now_utc() {
            /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
        }
    }

    if let Some(ref whitelist) = entity.ip_whitelist {
        if !ip_whitelist::evaluate(whitelist, &self.peer_ip) {
            /* log + */ return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
        }
    }

    let user_id = entity.user_id;
    self.log_auth_success(&entity, &fp).await;
    self.entity = Some(AuthedEntity { entity, user_id });

    Ok(Auth::Accept)   // line 396
}

// Called after successful authentication — send welcome message and set up session channel.
async fn auth_succeeded(&mut self, session: &mut Session) -> Result<(), Self::Error> {   // line 400
    let Some(ref authed) = self.entity else { return Ok(()); };
    let entity = authed.entity.clone();
    let entity_name = entity.name.as_deref().unwrap_or("(unnamed)").to_string();
    let entity_type = entity.entity_type.clone();
    let conn_id = self.conn_id;

    let handle = session.handle();

    // Register session entry (without channel yet — filled in below if open succeeds)
    {
        let mut reg = self.session_registry.lock().await;
        reg.insert(conn_id, SessionEntry { handle: handle.clone(), session_channel_id: None });
    }

    // Try to open a server-initiated session channel to the client for push messages.
    match handle.channel_open_session().await {      // <-- line ~419: THIS HANGS FOREVER
        Ok(ch) => {
            let ch_id = ch.id();
            self.session_channel_id = Some(ch_id);
            if let Some(entry) = self.session_registry.lock().await.get_mut(&conn_id) {
                entry.session_channel_id = Some(ch_id);
            }
            let welcome = format!(/* MLP-themed welcome text */);
            let _ = handle.data(ch_id, welcome.into_bytes()).await;

            let ping_handle = handle.clone();
            tokio::spawn(async move { /* keeps ch alive, sends Derpy ping every 5 min */ });
        }
        Err(e) => { tracing::debug!(%conn_id, err = %e, "SSH: could not open session channel"); }
    }

    let connect_msg = format!("📡 {} ({}) connected.", entity_name, entity_type);
    broadcast(&self.session_registry, &connect_msg, Some(conn_id)).await;

    Ok(())
}
```

Note: `auth_publickey` itself has **no missing return path** — every branch returns explicitly. The problem is downstream, in `auth_succeeded`, which russh calls automatically right after `Auth::Accept` (see §2).

---

### 2. The deadlock — `Mutex`es are fine, `Handle::channel_open_session()` is not

`self.session_registry.lock().await` (`tokio::sync::Mutex`) is acquired, used, and dropped in a scoped block *before* `channel_open_session()` is called — it is not re-entered, so it is not the culprit.

The real deadlock is architectural, inside russh 0.61 itself:

- `Session::run` (`~/.cargo/registry/.../russh-0.61.2/src/server/session.rs:478-676`) drives **one single `tokio::select!` loop per connection** with (among others) two mutually-exclusive branches:
  - `r = &mut reading => { ... reply(&mut self, &mut handler, &mut pkt).await ... }` — reads/decodes an incoming packet and dispatches it to the `Handler` (this is what calls `auth_publickey`, then, since state becomes `Authenticated`, calls `handler.auth_succeeded(self).await`, see `encrypted.rs:100-106`).
  - `msg = self.receiver.recv(), if !self.kex.active() => { ... Some(Msg::ChannelOpenSession { channel_ref }) => { let id = self.channel_open_session()?; ... } ... }` — this is the **only place** that actually performs the server-side channel-open handshake and produces the `ChannelMsg::Open` confirmation.
- `Handle::channel_open_session()` (`session.rs:250-262`) sends a `Msg::ChannelOpenSession` through `self.sender` (a bounded mpsc feeding `self.receiver` above) and then `.await`s a reply on a *different* channel via `wait_channel_confirmation` (`session.rs:379-410`), which just does `receiver.recv().await` in a loop until it sees `ChannelMsg::Open`.
- Because the `reading` branch and the `self.receiver.recv()` branch are two arms of the **same** `tokio::select!` on the **same** task, the task can only be executing one at a time. While it is inside `reply().await → handler.auth_succeeded().await → handle.channel_open_session().await → wait_channel_confirmation().await`, it is *not* polling `self.receiver` — so the `Msg::ChannelOpenSession` that was just enqueued is never dequeued, `Session::channel_open_session()` (the internal method that would actually send `SSH_MSG_CHANNEL_OPEN` and register the confirmation) is never invoked, and the confirmation the handler is waiting for **never arrives**. This is a permanent self-deadlock — the connection is stuck forever, with no timeout.

This is a genuine "await on something that never resolves," not a `Mutex` issue.

### 3. Effect on the wire — matches the reported "no accept, no reject, nothing"

`Session::run`'s loop only calls `self.flush()` / `packet_writer.flush_into(&mut stream_write).await` **after** the `tokio::select!` block for that iteration completes (`session.rs:643-650`). Since the iteration never completes (it's stuck inside `auth_succeeded`), the `SSH_MSG_USERAUTH_SUCCESS` packet that was already buffered by `server_read_auth_request` (`encrypted.rs:889`, called just before `auth_succeeded`) is **never flushed to the socket**. The client therefore receives literally nothing after its signed publickey request — exactly the symptom described. Any subsequent client packets (e.g. the `tcpip-forward` global request for `-R`) also just sit unread, since the `reading` future for the next packet isn't re-armed until the stuck select arm finishes.

### 4. Database queries — not the cause, but reviewed

`SshKey::find_by_fingerprint` (`crates/tunnel2tunnel-core/src/models/ssh_key.rs:25-36`) and `Entity::find_by_id_only` (`crates/tunnel2tunnel-core/src/models/entity.rs:43-54`) are plain `SELECT ... fetch_optional(pool)` calls against a `PgPoolOptions::new().max_connections(10)` pool (`crates/tunnel2tunnel-core/src/db.rs:5-6`). Nothing here holds a transaction open or exhausts the pool. `ConnectionLog::create` (`crates/tunnel2tunnel-core/src/models/connection_log.rs:26-55`) is a simple `INSERT ... RETURNING *`. None of these can hang indefinitely under normal operation and are not implicated.

### 5. `Auth::Accept` / `Auth::Reject` return-path audit

Every branch of `auth_publickey` (lib.rs:301-397) returns explicitly (`Ok(Auth::Reject{...})` ×7, or falls through to `Ok(Auth::Accept)` at line 396). There is no missing-return path. The bug is not in `auth_publickey`'s control flow but in the follow-on `auth_succeeded` callback that russh invokes automatically once `Auth::Accept` is returned.

### 6. Server slot map (`ServerSlots = Arc<Mutex<HashMap<(Uuid, u32), Handle>>>`)

Defined at lib.rs:40, populated in `tcpip_forward` (lib.rs:526-551) and cleared in `cancel_tcpip_forward` (lib.rs:553-575) — **not** touched during `auth_publickey`/`auth_succeeded` at all. It is irrelevant to this hang; the connection never even reaches `tcpip_forward` because it's already stuck in `auth_succeeded` beforehand. (The lock pattern there — `self.server_slots.lock().await.insert(...)` — is a normal short-held `tokio::sync::Mutex` use and is not itself deadlock-prone.)

### 5. Git history

```
7db0eb2 [backend] ssh: ai: Run: Items 1–4 from todo.md: welcome message, Derpy ping, broadcast notifications, chat relay.   <-- introduces auth_succeeded + the bug
6f36763 [backend|frontend] settings: ai: Run: Show SSH host key fingerprint in the Settings page.
e1cd51d [docker|backend] ssh host key: ai: Run: Persist SSH host key to disk to survive restarts.
e042099 [backend] ssh: ai: Run: Added info-level logging to all direct-tcpip rejection branches.
fc1aa4a [backend] ssh: ai: Run: Added verbose info-level logging for all auth methods and connection lifecycle.
9bb7cd6 [tunnel2tunnel] phase-4 ssh-server: ai: Run: Auth-gated russh tunnel router with fail2ban + IP whitelist.
47ee967 [tunnel2tunnel] phase-1 scaffold+auth: ai: Run: Add Rust workspace, migrations, Axum API, and Vue 3 login SPA.
```

`auth_succeeded` did not exist before `7db0eb2` — prior to that commit, `russh` would have called the default no-op `Handler::auth_succeeded` and the connection would have proceeded normally. This is a clear regression introduced by that commit.

### Suggested fix direction (for your review, not applied)

Don't call `handle.channel_open_session().await` (or any other `Handle` method that waits for a russh-processed reply) synchronously inside `auth_succeeded`. Instead, `tokio::spawn` a task that performs the channel-open-and-welcome sequence after `auth_succeeded` returns, e.g.:

```rust
async fn auth_succeeded(&mut self, session: &mut Session) -> Result<(), Self::Error> {
    ...
    let handle = session.handle();
    tokio::spawn(async move {
        match handle.channel_open_session().await { ... }
    });
    Ok(())
}
```

That lets `Session::run`'s select loop return to polling `self.receiver` (and flush `USERAUTH_SUCCESS`) immediately, so the spawned task's `channel_open_session().await` can actually be serviced by that same loop on a later iteration.