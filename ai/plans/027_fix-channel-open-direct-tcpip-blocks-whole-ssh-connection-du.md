# Fix: `channel_open_direct_tcpip` blocks whole SSH connection during server-liveness wait

## Context

User's VNC tunnel (`ssh -L 5951:m1n:5900 -L 5952:PC6:5900 ...`) got stuck on "Connecting..." even
though the server-side t2t log showed `direct-tcpip: bridge established` for `m1n`. Root cause
traced through two research passes:

1. `channel_open_direct_tcpip` (`crates/tunnel2tunnel-ssh/src/lib.rs:1064-1293`) contains a
   **12-second retry-wait loop** (250ms poll, `lib.rs:1214-1233`) that runs directly inside the
   async `Handler` trait method, before it returns `Ok(bool)`.
2. Per this repo's existing CLAUDE.md gotcha and confirmed against the vendored `russh-0.61.2`
   source: **all requests on one SSH connection are processed serially on a single per-connection
   task** (`server/session.rs:478-606`, one `tokio::select!` loop). Awaiting/sleeping inline in a
   `Handler` callback blocks that entire loop — no other channel-open requests, no data delivery
   on already-bridged channels, and no keepalive processing can proceed on that connection until
   the callback returns.
3. In the user's session, two sequential `PC6` requests each burned the full 12s retry window
   (24s total) before the `m1n` request — for a *different* forwarded port on the *same*
   connection — was even processed. That serialization is what stalled the VNC client.
4. Separately (found by the first research pass): `channel_open_forwarded_tcpip(...)`'s `Err`
   case (`lib.rs:1249-1252`) is propagated via a bare `?` with **no log line at all**, and per the
   russh source this makes the whole connection's `reply()` call return `Err`, most likely killing
   the entire SSH session rather than cleanly rejecting just that one channel. Worth fixing
   alongside the main issue since it's the same function.
5. Confirmed there is **no russh API to defer a channel-open confirmation** — the `Ok(bool)`
   returned by the handler is consumed synchronously by the caller in the same call frame
   (`server/encrypted.rs:1605-1621`) to send `CHANNEL_OPEN_CONFIRMATION`/`FAILURE` immediately.
   So "spawn a task and confirm later via some Session method" is not an option — this needs a
   different fix.

**Approach**: keep only the *fast* checks (auth, hostname resolution, port_config, subscription,
access-rule check — steps 1–3 in the function, `lib.rs:1073-1207`) as the synchronous gate for the
`Ok(bool)` decision. These are just DB lookups and are not expected to block for seconds. Move the
server-liveness retry-wait and the channel/bridge setup (current steps 4, `lib.rs:1214-1292`) into
a `tokio::spawn`ed task, and return `Ok(true)` immediately once steps 1–3 pass. This trades
"reject-before-confirm on timeout" for "confirm-then-close-on-timeout" (unavoidable given russh's
API), but keeps the 12s server-liveness grace period without blocking the connection for anything
else happening on it — including already-open VNC bridges.

## Changes

All in `crates/tunnel2tunnel-ssh/src/lib.rs`.

### 1. Make `bridges` / `bridge_ids` shareable across tasks

Currently plain `HashMap` fields on `T2tHandler` (lines 291, 295), mutated only from `&mut self`
methods that (per the confirmed serial-per-connection model) never previously ran concurrently.
Once bridge setup happens in a spawned task, it needs to write into these maps from outside
`&mut self`. Follow the exact pattern already used for `server_slots`/`active_tunnels`
(`ServerSlots`/`ActiveTunnels` type aliases, `lib.rs:43,64`, both `Arc<Mutex<HashMap<...>>>`):

- Add `pub type Bridges = Arc<Mutex<HashMap<ChannelId, (Handle, ChannelId)>>>;` and
  `pub type BridgeIds = Arc<Mutex<HashMap<ChannelId, Uuid>>>;` near the existing type aliases.
- Change the two struct fields (`lib.rs:291,295`) to these types.
- Update construction (`lib.rs:261-262`, currently `HashMap::new()`) to
  `Arc::new(Mutex::new(HashMap::new()))`.
- Update every existing access to lock first:
  - `data()` handler's `self.bridges.get(&channel)` → `self.bridges.lock().await.get(...)`.
  - `channel_eof()` (`lib.rs:1334-1343`) — same.
  - `channel_close()` (`lib.rs:1345-1357`) — `self.bridges.remove(...)` and
    `self.bridge_ids.remove(...)` → lock first.
  - `Drop for T2tHandler`'s bridge-cleanup block (`lib.rs:1403-1416`) — this reads `bridge_ids`
    synchronously today, which no longer works on a `Mutex`. Change it to unconditionally spawn
    (drop the `if !self.bridge_ids.is_empty()` sync check) an async task that locks `bridge_ids`,
    drains the ids, then locks `active_tunnels` to remove them — mirroring the existing
    `connection_log_ids` cleanup block's spawn style just above it.

### 2. Split `channel_open_direct_tcpip` at the step-3/step-4 boundary

Keep lines `1064-1207` (auth check through the cross-account `EntityAccess::check_access` gate)
exactly as-is — same `Ok(false)` + log-line behavior on every failure branch.

Replace the current step 4 block (`lib.rs:1214-1292`, the retry-wait loop through the
`"bridge established"` log) with:

```rust
let handle = session.handle();
let server_slots = self.server_slots.clone();
let bridges = self.bridges.clone();
let bridge_ids = self.bridge_ids.clone();
let active_tunnels = self.active_tunnels.clone();
let peer_ip = self.peer_ip.clone();

tokio::spawn(async move {
    let client_ch_id = channel.id();

    const POLL_INTERVAL: Duration = Duration::from_millis(250);
    const MAX_WAIT: Duration = Duration::from_secs(12);
    let deadline = tokio::time::Instant::now() + MAX_WAIT;
    let server_slot = loop {
        let found = server_slots.lock().await.get(&(target_entity_id, port_to_connect)).cloned();
        if found.is_some() { break found; }
        if tokio::time::Instant::now() >= deadline { break None; }
        tokio::time::sleep(POLL_INTERVAL).await;
    };

    let Some((server_handle, registered_address)) = server_slot else {
        tracing::info!(
            target_entity = %target_entity_id,
            target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
            proxy_port = port_to_connect,
            "direct-tcpip: rejected — target server never came online within the retry window"
        );
        let _ = channel.close().await;
        return;
    };

    let server_ch = match server_handle
        .channel_open_forwarded_tcpip(&registered_address, port_to_connect, "127.0.0.1", 0)
        .await
    {
        Ok(ch) => ch,
        Err(e) => {
            tracing::error!(
                err = ?e,
                target_entity = %target_entity_id,
                proxy_port = port_to_connect,
                "direct-tcpip: forwarded-tcpip open failed"
            );
            let _ = channel.close().await;
            return;
        }
    };

    let server_ch_id = server_ch.id();
    bridges.lock().await.insert(client_ch_id, (server_handle.clone(), server_ch_id));

    let bridge_id = Uuid::now_v7();
    active_tunnels.lock().await.insert(bridge_id, ActiveTunnelInfo {
        client_entity_id: client_entity.id,
        client_user_id,
        target_entity_id,
        port_config_id: port_config.id,
        proxy_port: port_to_connect,
        peer_ip,
        since: time::OffsetDateTime::now_utc(),
    });
    bridge_ids.lock().await.insert(client_ch_id, bridge_id);

    tokio::spawn(forward_channel(server_ch, handle.clone(), client_ch_id));

    tracing::info!(
        client_entity = %client_entity.id,
        target_entity = %target_entity_id,
        target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
        host = host_to_connect,
        port = port_to_connect,
        client_ch = %client_ch_id,
        server_ch = %server_ch_id,
        "direct-tcpip: bridge established"
    );
});

Ok(true)
```

Notes on capture: `channel` (the `Channel<Msg>` parameter) moves into the spawned task —
confirmed via research that russh's confirmation send (`finalize_channel_open`) does not need the
`Channel<Msg>` object itself, only the `*allowed` bool returned by this function, so moving it
away before returning is safe. `host_to_connect` is a `&str` borrowed from the caller's stack frame
— it must be turned into an owned `String` before moving into the `async move` block (add
`let host_to_connect = host_to_connect.to_string();` before the spawn, shadowing the parameter).
`client_entity`, `target_entity`, `port_config` are already owned/cloned locally earlier in the
function (verify `Entity`/`PortConfig` derive `Clone` — `PortConfig` confirmed at
`tunnel2tunnel-core/src/models/port_config.rs:13`; `Entity` should be checked the same way, clone
it explicitly if needed since the original code already treats `target_entity` as owned).

### 3. `forward_channel` — no change needed

`crates/tunnel2tunnel-ssh/src/lib.rs:1453-1472` is already a standalone spawned task; unaffected.

## Why the existing `tunnel_e2e` test didn't catch this

`crates/t2t/tests/tunnel_e2e.rs` only ever drives **separate SSH connections** — its two `ssh -L`
legs (`spawn_ssh_l`, real OpenSSH subprocesses) are each their own TCP+SSH connection, and its only
concurrency (`tokio::join!` around its two tunnels) is across those separate connections, not two
channel-open requests racing on *one* connection/`Session`. Since the bug is specifically that one
connection's single-task event loop serializes everything on *that* connection, a test that never
puts two direct-tcpip requests on the same connection structurally cannot see it — regardless of
timeout generosity (the file uses 5–15s polling windows, well over the 12s stall, so even a
coincidental slowdown wouldn't fail an assertion). No existing test also exercises the "target
port_config/subscription/access all valid, but no server ever registered" case at all — that path
exists in the source (`lib.rs:1235-1243`) but is currently untested.

`crates/t2t/tests/tarpit_e2e.rs` already has the right shape for a same-connection test: it drives
an in-process `russh::client::Handle<TestClient>` directly (`connect_client`/`connect_client_at`,
`crates/t2t/tests/tarpit_e2e.rs:147-175`) instead of shelling out to `ssh`, which is exactly what's
needed here since `ClientHandle` is cheaply cloneable and its
`channel_open_direct_tcpip(host, port, originator_address, originator_port)` method
(`russh-0.61.2/src/client/mod.rs:718`) can be called twice concurrently from the same handle.

## New regression test

Add to `crates/t2t/tests/tunnel_e2e.rs` (reuse its DB/fixture helpers — `User::create`,
`Entity::create`, `SshKey::create`, `PortConfig::create`, `PortSubscription::create`,
`EntityAccess::create` if cross-account — plus `spawn_server`/`connect_client`-style helpers ported
from `tarpit_e2e.rs`, since `tunnel_e2e.rs` doesn't have an in-process client harness yet):

`same_connection_online_target_not_blocked_by_offline_target_timeout`:

1. Create one client entity + key, two target ("server") entities + keys, all under one user (or
   a granted cross-account setup — same-account is simpler and sufficient here).
2. For target A ("online"): create its `PortConfig` (`enabled=true`, arbitrary `proxy_port`), a
   `PortSubscription` linking the client to it, and actually register the server side — either by
   spawning a real `ssh -R` subprocess for target A (as `tunnel_e2e.rs` already does elsewhere), or
   more simply by driving target A's own in-process `russh::client` connection through
   `tcpip_forward` directly (check `lib.rs`'s `tcpip_forward` handler signature — should be
   callable via `ClientHandle::tcpip_forward(address, port)` per the russh client API) so the
   `server_slots` map has a live entry for `(target_a_id, proxy_port_a)` before the test proceeds.
3. For target B ("offline"): create its `PortConfig` + `PortSubscription` the same way, but never
   register anything for it — no `-R`, no `tcpip_forward` call. This exercises the previously
   untested "never came online within the retry window" path deliberately, on demand, without
   waiting on a real timeout race.
4. Connect ONE client `ClientHandle` (`connect_client(port)` style, authenticated via the client's
   key) — this is the single connection both requests must share.
5. Using that one `handle`, fire both requests concurrently:
   ```rust
   let (online, offline) = tokio::join!(
       handle.channel_open_direct_tcpip(target_a_id.to_string(), proxy_port_a as u32, "originator", 0),
       handle.channel_open_direct_tcpip(target_b_id.to_string(), proxy_port_b as u32, "originator", 0),
   );
   ```
6. Assert `online` resolves to `Ok(channel)` **within well under the 12s retry window** — e.g.
   wrap the whole `join!` in `tokio::time::timeout(Duration::from_secs(3), ...)` and assert it
   doesn't time out, or record `Instant::now()` before and assert `online`'s completion happened
   in under ~2s. Before the fix, `online` would be stuck behind `offline`'s full 12s wait on the
   shared connection; after the fix, it resolves immediately (steps 1–3 pass fast, `Ok(true)`
   returns right away) independent of `offline`'s outcome.
7. Assert `offline` eventually fails/closes (either `Err` from `channel_open_direct_tcpip`, since
   russh's `finalize_channel_open` still sends `CHANNEL_OPEN_FAILURE` when steps 1–3 fail — note
   this test's B path passes steps 1–3, so post-fix it will actually get `Ok(true)` immediately too,
   then the spawned task closes the channel ~12s later once the retry window lapses; assert the
   resulting `Channel`'s `wait()` yields `ChannelMsg::Close`/`Eof`/connection drop within ~13s, not
   `Ok(bool)` — adjust the assertion to match whichever behavior the implementation actually
   produces once step 2 above is written, since this is the "confirm-then-close-on-timeout"
   trade-off documented above).
8. Optionally, also directly unit/integration-test the previously-silent `channel_open_forwarded_tcpip`
   error path (`lib.rs:1249-1252` pre-fix) by registering target A's server slot and then closing
   its underlying connection/`Handle` before the client's request reaches that line, then asserting
   an `error!`-level log line is emitted (e.g. via a `tracing` test subscriber capturing output) —
   this is a secondary, lower-priority addition since it's a logging gap, not the main blocking bug.

## Verification

1. `cargo build -p tunnel2tunnel-ssh -p t2t` (and full workspace) to confirm the `Arc<Mutex<>>`
   field changes don't break other call sites, and the moved/spawned block borrow-checks (in
   particular that `channel`, `host_to_connect`, and all captured locals satisfy `'static` for
   `tokio::spawn`).
2. Run the new test **against the pre-fix code first** (temporarily, or just reason about it) to
   confirm it actually fails/times out there — proving it would have caught this bug — then run it
   against the fixed code and confirm it passes quickly.
3. Run the existing `tunnel_e2e` and `tarpit_e2e` test suites — both should still pass unmodified,
   since normal bridging behavior (server already online, single-connection-at-a-time cases) is
   unchanged, just deferred by one tick through the spawn.
4. Manually reproduce the original bug scenario locally with real `ssh -L`/`-R` (the user's exact
   setup): one target online, one target with no server registered, both requested via `-L` on the
   same `ssh` invocation — confirm the online target's tunnel now bridges immediately instead of
   waiting behind the offline target's full 12s timeout.
5. Check logs: confirm the previously-silent `channel_open_forwarded_tcpip` error path now emits
   an `error!` line if forced (e.g. by killing the server-side SSH session between it registering
   its slot and the client's channel-open request).
