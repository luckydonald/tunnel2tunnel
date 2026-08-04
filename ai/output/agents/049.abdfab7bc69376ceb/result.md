Now I have the full picture. Here's the report.

## 1. The push mechanism itself — event-driven "wake up", not real diffing, plus a polling fallback

The core primitive is a payload-less broadcast channel:

`/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs:71`
```rust
pub type LiveUpdateTx = broadcast::Sender<()>;
```
`crates/tunnel2tunnel-ssh/src/lib.rs:75-77`:
```rust
pub fn new_live_update_tx() -> LiveUpdateTx {
    broadcast::channel(16).0
}
```

It's constructed once in `crates/t2t/src/main.rs:89` and handed to both the SSH server (`start_ssh(... live_update_tx ...)`) and the web `AppState` (`crates/tunnel2tunnel-web/src/lib.rs:35`), so both sides share the same `Arc`-backed `broadcast::Sender`.

The consumer side is `run_loop` in `crates/tunnel2tunnel-web/src/routes/live_ws.rs:129-171`:
```rust
async fn run_loop<T, F, Fut>(who: &str, mut socket: WebSocket, state: AppState, build: F)
where
    T: Serialize,
    F: Fn(AppState) -> Fut,
    Fut: std::future::Future<Output = Result<T, crate::WebError>>,
{
    let mut rx = state.live_update_tx.subscribe();
    let mut interval = tokio::time::interval(FALLBACK_RESYNC_INTERVAL); // 20s, line 35
    ...
    loop {
        // build(state.clone()) always re-queries the DB + locks the in-memory
        // maps FROM SCRATCH and sends the FULL rebuilt snapshot — no diffing.
        ...
        tokio::select! {
            _ = interval.tick() => {}      // fallback poll every 20s
            recv = rx.recv() => { ... }    // event-driven wakeup
            msg = socket.recv() => { ... } // detect client close
        }
    }
}
```

So: **it is event-driven from the SSH side for the vast majority of transitions**, with a 20s poll only as a fallback for state changes the notifier doesn't cover (the comment at `live_ws.rs:31-35` explicitly calls out `port_subscriptions` toggled via the HTTP API as the gap case). Critically: **the signal itself carries zero information** — `broadcast::Sender<()>` — every `.send(())` just means "something, somewhere, changed; go rebuild your full snapshot and resend it." There is no discrete event type, no diff, no entity/service scoping in the signal. Any of the 3 websocket routes wake up on *any* mutation anywhere in the system and rebuild+resend their own (possibly unrelated) snapshot.

This confirms your framing: today's mechanism is "state changed, go re-fetch everything" — there is no server-side diff computation anywhere. To build discrete toast events you'd need a new, richer channel (e.g. `broadcast::Sender<LiveEvent>` or similar) alongside/replacing this one, with the diff computed at the exact mutation point (see below) since that's the only place old-vs-new is naturally available.

## 2. SSH-side mutation points — file:line for each transition, all in `crates/tunnel2tunnel-ssh/src/lib.rs`

| Transition | Location | Notes |
|---|---|---|
| **Entity goes online** (auth success) | `log_auth_success`, line **459-460** | `tarpit::record_auth_success(...)`, then `let _ = self.live_update_tx.send(());`. This is *before* any port/tunnel state exists — just "this entity's SSH session now exists." |
| **Port starts forwarding** (`tcpip_forward`, i.e. `-R port:...`) | `tcpip_forward` handler, insert at **1049-1052**, send at **1053** | `self.server_slots.lock().await.insert((entity_id, *port), (session.handle(), address.to_string()));` then `live_update_tx.send(())`. Also auto-creates a `port_configs` row if missing (lines 1013-1047) — that's a DB-only state change with no separate notify (relies on this same send). |
| **Port stops forwarding** (`cancel_tcpip_forward`) | line **1065-1084**, remove at 1080-1083, send at **1084** | `self.server_slots.lock().await.remove(&(authed.entity.id, port));` |
| **Bridge established / bridging starts** (`channel_open_direct_tcpip`, fast path — target already registered) | insert into `active_tunnels` at **1293-1306**, send at **1307** | This is the "active" transition — `ActiveTunnelInfo` inserted keyed by a fresh `Uuid::now_v7()` bridge id. |
| **Bridge established (slow path** — target not yet registered, up to 12s retry loop in a spawned task**)** | insert at **1416-1429**, send at **1430** | Same shape, inside the `tokio::spawn` block starting at line 1352. |
| **Bridge closes** (`channel_close`) | line **1500-1513**: remove bridge at 1505-1507, remove `active_tunnels` entry + send at **1508-1511** | `if let Some(bridge_id) = self.bridge_ids.lock().await.remove(&channel) { self.active_tunnels.lock().await.remove(&bridge_id); let _ = self.live_update_tx.send(()); }` |
| **Entity goes offline / connection dropped** (any reason — clean close, abrupt drop, auth failure with prior log rows) | `impl Drop for T2tHandler`, lines **1516-1601**, three separate spawned cleanup tasks each firing their own `live_update_tx.send(())`: <br>• server_slots cleanup: **1550-1559** (send at 1557)<br>• active_tunnels/bridge cleanup: **1561-1583** (send at 1580, only if `!ids.is_empty()`)<br>• connection_log `ended_at` stamping: **1585-1600** (send at 1598) | Three independent spawned tasks per disconnect, each racing to notify — this is why the receiver side treats `Lagged` as "just resync," per `run_loop`'s comment at `live_ws.rs:155-159`. |

No Postgres LISTEN/NOTIFY anywhere — I grepped and found none. No polling loop drives correctness; the 20s interval in `live_ws.rs` is explicitly a fallback for the one known gap (HTTP-driven `port_subscriptions.enabled` toggles, which don't touch the SSH handler at all and thus never call `live_update_tx.send()`).

## 3. Server-side "current live status" data structures — in `tunnel2tunnel-ssh`, not `tunnel2tunnel-core`

Two `Arc<Mutex<HashMap<...>>>`s, both defined in `crates/tunnel2tunnel-ssh/src/lib.rs`:

**`ServerSlots`** — line 43:
```rust
pub type ServerSlots = Arc<Mutex<HashMap<(Uuid, u32), (Handle, String)>>>;
```
Key: `(entity_id, proxy_port)`. Value: `(russh Handle, registered_address)`. Presence = "this entity has this port actively forwarded right now" (drives `port_forwarded` / `NotForwarded` vs `Idle`/`Active` in `remote_status_for`).

**`ActiveTunnels`** — line 64, keyed by a synthetic bridge id (not by `(entity,port)`, since multiple subscribers can bridge the same service concurrently):
```rust
pub type ActiveTunnels = Arc<Mutex<HashMap<Uuid, ActiveTunnelInfo>>>;

#[derive(Clone, Debug)]
pub struct ActiveTunnelInfo {
    pub client_entity_id: Uuid,
    pub client_user_id: Uuid,
    pub target_entity_id: Uuid,
    pub port_config_id: Uuid,
    pub proxy_port: u32,
    pub peer_ip: String,
    pub since: time::OffsetDateTime,
}
```

Plus `RemoteStatus` (the derived 4-state enum), in `tunnel2tunnel-web/src/routes/live_connections.rs:70-77`:
```rust
pub enum RemoteStatus { Offline, NotForwarded, Idle, Active }
```
computed by `remote_status_for(enabled, online, port_forwarded, bridge_active)` at `live_connections.rs:446-455` — this is a *pure function of current values only*, called fresh every snapshot build; it has no memory of the previous state, so it cannot itself detect "transitioned from idle to active" — that has to be computed by whoever calls it, or (better) at the mutation point in `tunnel2tunnel-ssh` where old/new values are both naturally in hand.

**Diffability assessment**: yes, this is very diffable at the mutation point, because every mutation site already has exactly the "old vs new" information needed for a discrete event, before it's discarded:
- `tcpip_forward`/`cancel_tcpip_forward`: insert/remove on `ServerSlots` keyed by `(entity_id, proxy_port)` — trivial to know "this specific (entity, port) just started/stopped forwarding" (`port_forwarding_started` / `port_forwarding_stopped`, with `entity_id` + `proxy_port`, and you already have `entity_name` and the `PortConfig` row loaded right there at lines 1004-1047).
- `channel_open_direct_tcpip` bridge-established / `channel_close` bridge-removed: you have `client_entity_id`, `target_entity_id`, `port_config_id`, `proxy_port` right in `ActiveTunnelInfo` at the exact insert (1293-1306, 1416-1429) and remove (1508-1511) sites — trivial to emit `bridge_started`/`bridge_stopped { client_entity_id, target_entity_id, port_config_id, service_name }`.
- Auth success/Drop (entity online/offline): you have `entity.id`/`entity.name` right there too.

What's *not* diffable at the mutation point without extra plumbing: the aggregated `live` flag on a service row (`true` iff *any* subscriber has a bridge) and the composite `remote_status` enum — those are computed later, in `tunnel2tunnel-web`, by joining `ServerSlots`/`ActiveTunnels` with DB rows (`build_entity_live_connections`, `live_connections.rs:129-231`). If you want toast events phrased in terms of the frontend's existing `remote_status`/`live` vocabulary (e.g. "service X went idle→active"), you'd either (a) recompute `remote_status_for` old-vs-new around each raw mutation — doable, since all four inputs (`enabled`, `online`, `port_forwarded`, `bridge_active`) are derivable at each ssh-side mutation site, just currently split between `tunnel2tunnel-ssh` (session/port/bridge state) and `tunnel2tunnel-core`/DB (`enabled`, `online` via `ConnectionLog::entity_status`) — or (b) keep raw discrete events (`port_forwarding_started`, `bridge_started`, etc.) and let the frontend map those to toast copy directly, which avoids needing DB round-trips inside the SSH handler's hot path.

## 4. Per-user aggregation point — `/api/me/live-connections/ws` is already correctly scoped

`crates/tunnel2tunnel-web/src/routes/live_ws.rs:98-110`:
```rust
pub async fn my_live_connections_ws(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let who = format!("me:{}", user.id);
        run_loop(&who, socket, state, move |state: AppState| async move {
            build_my_live_connections(&state, user.id).await
        })
        .await;
    })
}
```
`AuthUser` extraction (session-based, see #5) gives `user.id`, and `build_my_live_connections` (`live_connections.rs:394-428`) does:
```rust
pub async fn build_my_live_connections(state: &AppState, user_id: Uuid) -> Result<Vec<DashboardRow>, WebError> {
    let entities = Entity::list_for_user(&state.db, user_id).await?;
    let mut rows = Vec::new();
    for entity in entities {
        let snapshot = build_entity_live_connections(state, entity.id).await?;
        ...
    }
    Ok(rows)
}
```
i.e. it enumerates *only* entities owned by that user (`Entity::list_for_user`), then aggregates both "my services" and "my subscriptions" rows across all of them into one flat list. This is exactly the right existing aggregation point/route to piggyback discrete toast events onto: one connection per logged-in user, already restricted to their own entities, already wired into the same `run_loop`/`live_update_tx` plumbing regardless of which frontend page/route is active (a route-agnostic per-user channel). No new per-user scoping logic needs inventing — this route already is that.

Caveat: currently this route (like the other two) only carries the rebuilt-snapshot type (`Vec<DashboardRow>`); it does not currently carry a discrete-event side-channel — that's the new piece to add (e.g. widen the message union to `{type: "snapshot", rows: [...]}` | `{type: "event", ...}`, or run a second logical message stream over the same socket).

## 5. Auth extraction in websocket upgrade handlers

`crates/tunnel2tunnel-web/src/extractors.rs`:
```rust
pub struct AuthUser(pub User);
pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state).await.map_err(|_| WebError::Unauthorized)?;
        let user_id: Option<Uuid> = session.get("user_id").await.map_err(|_| WebError::Unauthorized)?;
        let user_id = user_id.ok_or(WebError::Unauthorized)?;
        let user = User::find_by_id(&state.db, user_id).await.map_err(WebError::Core)?.ok_or(WebError::Unauthorized)?;
        if user.is_locked { return Err(WebError::Forbidden); }
        Ok(AuthUser(user))
    }
}
```
`AdminUser` (extractors.rs, same file) just wraps `AuthUser::from_request_parts` and additionally checks `user.is_admin`.

These are plain axum `FromRequestParts` extractors, so they work transparently in the WS upgrade handler signature — before `ws.on_upgrade(...)` runs, axum has already run session-cookie lookup → `user_id` → DB `User` fetch → lock check. This is identical to how normal HTTP routes authenticate (`tower_sessions` cookie session, `user_id` stored in the session on login). Used at:
- `crates/tunnel2tunnel-web/src/routes/live_ws.rs:73` (`entity_live_connections_ws`, plus a `require_owner` check at line 78 against the path's `entity_id`)
- `live_ws.rs:99` (`my_live_connections_ws`)
- `live_ws.rs:113` (`admin_live_connections_ws`, via `AdminUser`)

Routes wired at `crates/tunnel2tunnel-web/src/lib.rs:160-169`.

## Summary for the implementation plan

- The signal type to extend/replace is `LiveUpdateTx = broadcast::Sender<()>` (`tunnel2tunnel-ssh/src/lib.rs:71`) — currently zero-payload; a new parallel `broadcast::Sender<LiveEvent>` (or widen this one to carry an `Option<LiveEvent>`) is the natural place to plug in.
- Emit discrete events at the 8 existing mutation sites already enumerated in #2 — each already has the entity/port/bridge context needed for a payload like `{type: "port_forwarding_started", entity_id, proxy_port, service_name}` without extra DB queries.
- `/api/me/live-connections/ws` (`live_ws.rs:98-110` + `build_my_live_connections`, `live_connections.rs:394-428`) is the correct existing per-user, route-agnostic aggregation point/channel to also carry these events — it's already scoped to `Entity::list_for_user(user_id)`.
- `run_loop` (`live_ws.rs:129-171`) would need restructuring since today it only knows how to rebuild-and-resend a full snapshot on any wakeup; it has no notion of "here's the specific delta that caused this wakeup" — that data currently dies at the `.send(())` call site and would need to travel through the channel payload instead.