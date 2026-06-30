# Plan: todo.md items

## Context

Seven items from `ai/todo.md`. Items 5–7 are frontend-only. Items 1–4 require SSH server changes to send messages to connected clients. Addressed in two groups.

---

## Group A — Frontend (items 5, 6, 7)

### Item 7 (bug): Access Rules table empty on initial page load

**Root cause:** `loadAccess()` is only triggered by clicking "Add rule". The guard `v-if="accessLoaded"` keeps the table hidden until then.

**Fix:** Call `loadAccess()` inside the existing `load()` function in `EntityDetailPage.vue` (around line 54), so it runs on `onMounted`. Remove the `v-else` placeholder ("Click 'Add rule' to manage access") since the table will now load eagerly.

**File:** `frontend/src/pages/EntityDetailPage.vue`

---

### Item 5: Clicking `-L <port>:<uuid>:<port>` navigates to the server's port section

**Context:** `SshCommandDisplay.vue` renders port flags as `<span class="cmd-flag">{{ portFlag(port) }}</span>` inside a `<pre>`. `portFlag()` (line 64) uses `port.server_entity_id` to build `-L local:server:proxy`.

**Plan:**
1. Add `id="ports"` to the ports section heading/element in `EntityDetailPage.vue` so deep-links with `#ports` scroll to it.
2. In `SshCommandDisplay.vue`, for client-entity ports with a `server_entity_id`, wrap the `<span class="cmd-flag">` in a `<RouterLink :to="'/entities/' + port.server_entity_id + '#ports'">`; keep the plain span for server entities and ports without a resolved server.

**Files:**
- `frontend/src/components/SshCommandDisplay.vue`
- `frontend/src/pages/EntityDetailPage.vue` (add `id="ports"`)

---

### Item 6: Display reversed ("incoming") access rules as a secondary table

Reversed = rules where this entity is the **subject** (`subject_entity_id = entity.id`), not the owner. Shows "who has granted access to me."

**Backend:**
1. `tunnel2tunnel-core/src/models/entity_access.rs` — add `EntityAccess::list_incoming(pool, subject_entity_id: Uuid)`: `SELECT * FROM entity_access WHERE subject_type = 'entity' AND subject_entity_id = $1 ORDER BY created_at DESC`
2. `tunnel2tunnel-web/src/routes/access.rs` — add handler `get_incoming_access` (reuse existing `list_access` pattern; no write access needed)
3. `tunnel2tunnel-web/src/lib.rs` — register `GET /api/entities/{entity_id}/access/incoming`

**Frontend:**
1. `frontend/src/api/friends.ts` — add `listIncomingAccess(entityId)` → `GET /api/entities/{id}/access/incoming`
2. `frontend/src/pages/EntityDetailPage.vue` — below the existing Access Rules section, add an "Incoming Grants" section:
   - Loads on mount (alongside existing `loadAccess()`)
   - Table columns: Owner entity | Hostname alias | Created
   - Owner entity shown as `EntityName` / link to `/entities/{owner_entity_id}`
   - No add/delete (read-only — you can't modify another entity's rules)

**Files:** `entity_access.rs`, `access.rs`, `lib.rs`, `friends.ts`, `EntityDetailPage.vue`

---

## Group B — SSH messaging (items 1–4)

All four features share the same underlying mechanism: a server-initiated session channel opened to each authenticated client, used for sending text and relaying chat. They are implemented together.

### Architecture

**New shared state in `T2tServer`:**
```rust
type SessionRegistry = Arc<Mutex<HashMap<Uuid, SessionEntry>>>;

struct SessionEntry {
    entity_id: Uuid,
    handle: Handle,
    session_channel: Option<ChannelId>,
}
```

**New fields in `T2tHandler`:**
```rust
conn_id: Uuid,                    // generated in new_client()
session_registry: SessionRegistry,
```

**Helper (free function):**
```rust
async fn broadcast(registry: &SessionRegistry, msg: &str, exclude: Option<Uuid>);
// Sends ANSI-coloured message to all registered session channels except `exclude`.
// Uses handle.data(ch_id, russh_cryptovec::CryptoVec::from_slice(msg.as_bytes()))
```

Check whether `tokio` workspace dep has the `time` feature; add it if missing for `tokio::time::sleep` in the ping task.

---

### Item 1: Welcome message (MLP:FiM themed)

**Hook:** implement `auth_succeeded(&mut self, session: &mut Session)` in `T2tHandler`.

```
Steps inside auth_succeeded:
1. Clone handle = session.handle()
2. Register in session_registry: conn_id → SessionEntry { entity_id, handle, session_channel: None }
3. Open session channel: let ch = handle.channel_open_session().await?
4. Write welcome text to ch (ANSI colours ok):
   "✨ Welcome to tunnel2tunnel, {entity_name}! ✨
    Your SSH tunnel is now active. Twilight has verified your access rules
    and everything checks out. The portal stands ready. 📚"
5. Store ch.id() back: session_registry entry session_channel = Some(ch.id())
6. Spawn channel-keeper task (see item 2) that holds `ch` alive
7. Broadcast arrival notice to all OTHER sessions (see item 3)
```

**File:** `crates/tunnel2tunnel-ssh/src/lib.rs`

---

### Item 2: Periodic ping (Derpy mail-mare themed)

Spawned inside `auth_succeeded` after step 5:

```rust
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(5 * 60)); // 5-min
    interval.tick().await; // skip first immediate tick
    loop {
        interval.tick().await;
        let msg = "\r\n✉ Derpy Hooves stopped by to make sure your tunnel is still up! 🧁 \r\n";
        if handle.data(ch_id, CryptoVec::from_slice(msg.as_bytes())).await.is_err() {
            break;
        }
    }
});
```

The channel-keeper task (holds `ch`):
```rust
tokio::spawn(async move {
    loop {
        match ch.wait().await {
            None | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
            _ => {}
        }
    }
});
```

---

### Item 3: Notifications on connect / disconnect / port events

**Connect:** broadcast at end of `auth_succeeded`:
```
"📡 {entity_name} ({entity_type}) connected."
```

**Disconnect:** in `Drop`, before removing from registry, broadcast to all remaining sessions:
```
"🔌 {entity_name} disconnected."
```

**Port becomes available** (`tcpip_forward`): broadcast to all sessions (they can check their own access):
```
"🟢 Port {port} on {entity_name} is now registered."
```

**Port goes away** (`cancel_tcpip_forward`):
```
"🔴 Port {port} on {entity_name} is no longer available."
```

Broadcasting is only done if the sender's entity is known (skip during unauthenticated drops).

---

### Item 4: Chat relay

**Mechanism:** when a client types into its terminal during an active `ssh -N` session, that data only reaches the server if it opened its own session channel. For the server-initiated session channel (item 1), `ssh -N` clients *can* receive data written to it but typically cannot send back (stdin not forwarded).

For **interactive** sessions (client connects without `-N` and sends a shell/exec request), implement `channel_open_session` to accept the channel and `data()` to relay.

**Plan:**
1. In `T2tHandler::data()`, add a branch: if `channel == session_registry entry for conn_id → session_channel`, treat the bytes as a chat line.
2. Implement `channel_open_session` to return `true` (accept it) and store the channel_id as the session channel (so clients connecting without `-N` also get a chat channel).
3. Chat relay logic in `data()`:
   ```rust
   let text = String::from_utf8_lossy(data).trim_end().to_string();
   if text.is_empty() { return Ok(()); }
   let sender_name = format!("{} ({})", entity_name, &entity_id.to_string()[..8]);
   let msg = format!("\r\n\x1b[36m{}: {}\x1b[0m\r\n", sender_name, text);
   broadcast(&self.session_registry, &msg, Some(self.conn_id)).await;
   ```

Because `T2tHandler` doesn't currently store the session_channel_id directly (it's in the registry), add a `session_channel: Option<ChannelId>` field to `T2tHandler` too, set alongside the registry entry.

---

## Verification

**Items 5–7 (frontend):**
- `npm run dev` in `frontend/`, visit an entity detail page
- Confirm Access Rules table shows immediately on load (item 7)
- Confirm a `-L` flag is a clickable link; clicking it navigates to the server entity's page and scrolls to the ports section (item 5)
- Confirm a second table "Incoming Grants" appears below Access Rules when another entity has granted access to this entity (item 6)

**Items 1–4 (SSH):**
- Start the server with `RUST_LOG=info cargo run -p t2t`
- Connect two entities: `ssh -N -R 2222:localhost:22 <entity_id>@localhost -p 2222`
- Confirm welcome message appears in each terminal on connect
- Wait 5 minutes, confirm Derpy ping appears
- Connect a second entity, confirm first terminal shows "📡 … connected" notification
- For chat: connect without `-N` and type a message; confirm it appears in the other session's terminal

---

## Implementation order

1. Item 7 (one-line bug fix)
2. Item 5 (frontend link — small)
3. Item 6 (backend + frontend — medium)
4. Items 1–4 together (SSH messaging — large, all share same new state)
