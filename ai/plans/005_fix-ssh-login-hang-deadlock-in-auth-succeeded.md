# Fix SSH login hang (deadlock in `auth_succeeded`)

## Context

Users can no longer establish tunnels (`ssh -N -R ... -p 2222`). A verbose client-side
capture shows key exchange and pubkey auth succeed all the way through
`SSH_MSG_USERAUTH_PK_OK`/signature acceptance, then the connection just hangs — no
`USERAUTH_SUCCESS`, no reject, nothing.

Root cause (confirmed by reading `crates/tunnel2tunnel-ssh/src/lib.rs:400-477` directly
and cross-checking russh 0.61.2's own source at
`~/.cargo/registry/src/.../russh-0.61.2/src/server/session.rs`):

- `auth_succeeded` (added in commit `7db0eb2`, "welcome message, Derpy ping, broadcast
  notifications, chat relay") calls `handle.channel_open_session().await` synchronously.
- Russh drives each connection with a single-task `tokio::select!` loop. One branch reads
  and dispatches incoming packets (this is what calls `auth_publickey` → `auth_succeeded`);
  another branch drains `self.receiver` to actually perform channel-open handshakes and
  deliver their confirmations.
- `channel_open_session()` sends a request into that same receiver and then awaits its
  confirmation. But the task is already inside the "dispatch" branch (running
  `auth_succeeded`), so it can never get back around to servicing its own request. Permanent
  deadlock, with no timeout. The already-buffered `USERAUTH_SUCCESS` packet is never
  flushed, matching the observed symptom exactly.
- This is a regression: before `7db0eb2`, `auth_succeeded` was a no-op and no code path
  awaited a self-serviced confirmation from within a `Handler` callback.

Every branch of `auth_publickey` itself returns correctly — the bug is entirely in the
follow-on `auth_succeeded` callback, not the credential-checking logic.

## Fix

**File: `crates/tunnel2tunnel-ssh/src/lib.rs`**

1. **`auth_succeeded` (lines 400-477):** keep the initial `session_registry` insert
   synchronous (cheap, no deadlock risk), but move the `channel_open_session().await` call
   and everything that depends on its result (setting the channel id, sending the welcome
   message, spawning the Derpy-ping keepalive loop) into a `tokio::spawn`. This lets the
   connection's event loop return immediately, flush `USERAUTH_SUCCESS`, and go on to
   service the spawned task's own channel-open request on a later iteration — no protocol
   behavior changes, just deferred to a separate task. The `broadcast(...)` arrival
   notification stays inline (it only does non-blocking `mpsc` sends via `Handle::data`,
   which does not deadlock).

2. **Registry becomes the sole source of truth for the push channel id.** Because the
   channel-open now happens inside a spawned task, `self.session_channel_id` (a field on
   `T2tHandler`, line 163) can no longer be set directly on `self` from there. Rather than
   leaving that field stale (which would silently break the "type to relay chat" feature
   for server-initiated push channels), remove the field and make the existing
   `SessionEntry.session_channel_id` in `session_registry` (already updated by both this
   path and the client-initiated `channel_open_session` handler) the single source of
   truth:
   - Remove `session_channel_id` field + its init (`lib.rs:163`, `lib.rs:137`).
   - Remove the now-orphaned write in the client-initiated `channel_open_session` handler
     (`lib.rs:492`) — the registry update two lines later (`495-497`) already covers it.
   - Change the chat-relay check in `data()` (`lib.rs:722`) from
     `Some(channel) == self.session_channel_id` to an async lookup:
     `self.session_registry.lock().await.get(&self.conn_id).and_then(|e| e.session_channel_id) == Some(channel)`.

3. **Document the gotcha in `CLAUDE.md`** under "Known gotchas" (matches the existing
   pattern for other russh 0.61 quirks like `Auth::Reject`):
   > **russh `Handle` confirmation calls** (e.g. `channel_open_session()`) must be
   > `tokio::spawn`ed, never `.await`ed inline from a `Handler` callback — the confirmation
   > round-trips through that same connection's single-task event loop, so awaiting it
   > synchronously inside the callback deadlocks permanently.

No other files need to change. `auth_publickey`, the DB queries, the IP whitelist check,
and the `server_slots` map are all unrelated to this bug (verified — no locks held across
awaits, no missing return paths).

## Verification

1. `cargo check -p tunnel2tunnel-ssh` (and `cargo build -p t2t` if disk space allows —
   heads up: local disk is currently almost full, ~149 MiB free on `/`, which errored out
   one of my own read-only shell commands mid-investigation; may need to be freed before a
   full build/link succeeds).
2. Run the server locally per the CLAUDE.md instructions (Podman Postgres + `cargo run -p
   t2t`), create a test entity/key via the web UI, and reproduce the exact repro command:
   `ssh -N -i <key> -R 5900:localhost:5900 <uuid>@localhost -p 2222`. Confirm it connects
   and stays connected (no hang), and that `tcpip_forward` succeeds afterward.
3. Optionally test an interactive (non `-N`) session too, to confirm the welcome message /
   chat-relay-via-registry path still works after the field removal.
4. This does *not* include pushing/deploying to the remote Coolify host
   (`tunnel2tunnel-d5oteit4omkq577f6j4r3rd9.h1.bn-x.de`) — that's a separate, higher-blast-radius
   step I'll confirm with you separately once the fix is verified locally.
