# Toast notifications for live websocket events

## Context

Live status (connections, port forwarding, bridging) already streams to the frontend over websockets, but each socket just re-pushes a *full state snapshot* on every backend change (`useLiveSocket.ts` overwrites local refs wholesale — no discrete "this specific thing changed" event anywhere). The user wants toast popups for concrete events — connection up/down, port forwarding start/stop, and bridging (idle↔active) — driven by real server-side pushes at the exact moment the state changes (not client-side diffing of snapshots, not a `GET` disguised as a websocket). Toasts must show up regardless of which page/route is open, scoped to the logged-in user's own entities.

While designing this, a broader unification fell out naturally and is in scope for this pass: today there are three near-duplicate live-status websocket routes (`/api/entities/{id}/...`, `/api/me/...`, `/api/admin/...`). `build_my_live_connections` (the `me` route's builder) **already internally calls** `build_entity_live_connections` per owned entity and then discards the nested detail (`services[].subscribers[]`, `entity_online`) to flatten into `DashboardRow`. Widening the payload to keep that nested detail lets `/api/entities/{id}/...` be retired — `EntityDetailPage.vue` becomes a client-side filter over data already synced by the one global store, making entity navigation instant. Taking this further: the admin route doesn't need to be a separate connection either — it's the *same* data, unfiltered instead of owner-filtered, which the connection can switch into on request. So all three collapse into **one route, one shape, one connection**, with a client→server command to flip between "mine" and "all" scope (admin-only for "all"), and a per-row `mine: bool` so the UI (and the toast logic specifically) can tell which rows are the viewer's own regardless of current scope.

Explicit decisions made during design (do not revisit without reason):
- **No server-side diff engine.** Every state transition already happens at a known, specific code location (SSH mutation site) where old/new state is directly in hand — emit the discrete event *there*, don't reconstruct it later by diffing two snapshots.
- **One message = full snapshot + optional `reason`.** Not separate `snapshot`/`event` message types — every push already is the full current state; `reason` just names *why* this particular push happened. `reason: None` (initial connect / 20s fallback resync / lagged-resync / scope switch) → state update only, no toast.
- **"Should I resend now" (scope-based) is a different question from "should this toast" (always viewer's-own-entities-based).** In `all` scope, any change anywhere resends (the admin table needs it live); but the frontend only ever toasts for rows where `mine == true`, regardless of current scope. These must not be conflated into one filter.
- **One route, one connection, scope switch via command frame**, not three routes/three connections. `/api/entities/{id}/...`, `/api/me/...`, and `/api/admin/...` are all retired in favor of a single `/api/live-connections/ws` — no `/me` in the path, since it's no longer mine-only once scope flips.

Existing infra confirmed reusable as-is: `frontend/src/composables/useToast.ts` (singleton `show(message, level, durationMs)`) + `frontend/src/components/ToastContainer.vue` (`Teleport to="body"`). Broadcast plumbing: payload-less `broadcast::Sender<()>` (`LiveUpdateTx`, `crates/tunnel2tunnel-ssh/src/lib.rs:71`) shared between the SSH server and web `AppState`, used today by all three routes' `run_loop` (`crates/tunnel2tunnel-web/src/routes/live_ws.rs:129-171`) as a bare "something changed, rebuild" wakeup — being replaced for this one surviving route by the richer `LiveEvent` channel below.

Frontend today opens the `me` socket locally inside `DashboardPage.vue`'s `onMounted`. `App.vue` remounts the routed component on every navigation (`<RouterView :key="route.path" />`), so anything living inside a page component is destroyed/recreated on route change — incompatible with "toasts independent of route." The socket must live somewhere that survives navigation: `App.vue` itself (never keyed/remounted), via a Pinia store.

## Approach

### Backend

1. **`crates/tunnel2tunnel-ssh/src/lib.rs`** — add alongside `LiveUpdateTx`:
   ```rust
   pub type LiveEventTx = broadcast::Sender<LiveEvent>;
   pub fn new_live_event_tx() -> LiveEventTx { broadcast::channel(64).0 }

   #[derive(Clone, Debug, Serialize)]
   #[serde(tag = "kind", rename_all = "snake_case")]
   pub enum LiveEvent {
       EntityOnline { entity_id: Uuid },
       EntityOffline { entity_id: Uuid },
       PortForwardingStarted { entity_id: Uuid, proxy_port: u32 },
       PortForwardingStopped { entity_id: Uuid, proxy_port: u32 },
       BridgeStarted { client_entity_id: Uuid, target_entity_id: Uuid, proxy_port: u32 },
       BridgeStopped { client_entity_id: Uuid, target_entity_id: Uuid, proxy_port: u32 },
   }
   impl LiveEvent {
       pub fn entity_ids(&self) -> Vec<Uuid> { /* every entity id mentioned */ }
   }
   ```
   Deliberately minimal payloads (ids + port only, no names) — the frontend already receives entity/service names via the snapshot on the same socket and enriches from a local lookup rather than the SSH hot path doing extra DB work.

2. Add a `live_event_tx: LiveEventTx` field to `T2tHandler` (threaded through its constructor next to `live_update_tx`). Emit one `let _ = self.live_event_tx.send(LiveEvent::…)` beside each existing `live_update_tx.send(())` call, using values already in scope at that call site:
   - auth success (`log_auth_success`, ~line 459) → `EntityOnline`
   - `tcpip_forward` (~1053) → `PortForwardingStarted`
   - `cancel_tcpip_forward` (~1084) → `PortForwardingStopped`
   - `channel_open_direct_tcpip` fast path (~1307) and the spawned slow-path task (~1430, clone `live_event_tx` into the spawn like `live_update_tx` already is) → `BridgeStarted`
   - `channel_close` (~1508-1511) → capture the `Option<ActiveTunnelInfo>` returned by `active_tunnels.lock().await.remove(&bridge_id)` (currently discarded) to get `client_entity_id`/`target_entity_id`/`proxy_port` → `BridgeStopped`
   - `Drop`'s three cleanup tasks (~1550-1600) → the entity-online-cleanup task emits `EntityOffline` (already has `entity_id`); the active-tunnels cleanup task captures each removed `ActiveTunnelInfo` → `BridgeStopped` per id; the `server_slots` cleanup task captures the `(entity_id, port)` keys `retain` is about to drop → `PortForwardingStopped` per port.
   This channel entirely replaces `live_update_tx` as the wake source for the one surviving route (step 5) — `live_update_tx` itself can stay for now (harmless if unused) or be removed once confirmed nothing else needs it after the entity/admin routes are deleted.

3. **`crates/tunnel2tunnel-web/src/lib.rs`** — add `live_event_tx: LiveEventTx` to `AppState`. **`crates/t2t/src/main.rs`** — construct via `new_live_event_tx()` next to `new_live_update_tx()`, pass to both `start_ssh(...)` and the web `AppState`.

4. **`crates/tunnel2tunnel-web/src/routes/live_connections.rs`** — unify around one query function and one shape, `EntityLiveSnapshot`:
   ```rust
   #[derive(Serialize)]
   pub struct EntityLiveSnapshot {
       pub entity_id: Uuid,
       pub entity_name: Option<String>,
       pub entity_online: bool,
       pub entity_last_disconnected_at: Option<OffsetDateTime>,
       pub mine: bool,                              // owning user == connection's user
       pub services: Vec<ServiceLiveStatus>,        // already exists, includes subscribers[]
       pub subscriptions: Vec<SubscriptionLiveStatus>, // already exists
       #[serde(skip_serializing_if = "Option::is_none")]
       pub account: Option<AccountRef>,              // admin-only: owning user (id/username)
   }
   ```
   `account` (and any other admin-only identifying field currently on `LiveConnectionRow`, e.g. `peer_ip` — **verify at implementation time** whether `peer_ip` belongs at the entity level or per-subscriber inside `services[].subscribers[]`/`LiveSubscriberInfo`, and place it there instead if so) is only ever populated when the query runs in `all` scope — never sent to a non-admin regardless, since only admins can request that scope (belt-and-suspenders, not the only guard).
   Add a `build_live_connections(state, user_id, scope: Scope) -> Vec<EntityLiveSnapshot>` that replaces `build_my_live_connections`/`build_admin_live_connections`: `Scope::Mine` → `Entity::list_for_user(user_id)` (today's behavior, `mine` always `true`, `account`/admin fields `None`); `Scope::All` → all entities, `mine` computed per row against `user_id`, admin fields populated. Internally still calls `build_entity_live_connections` per entity exactly as `build_my_live_connections` does today — this is "stop discarding computed detail before serializing" plus "broaden the entity list when scoped to All," not new query logic. Delete `DashboardRow`, `build_my_live_connections`, `build_admin_live_connections`, `build_entity_live_connections`'s standalone HTTP-exposed caller. Delete `/api/entities/{id}/live-connections/ws` and `/api/admin/live-connections/ws` plus their route registrations in `crates/tunnel2tunnel-web/src/lib.rs`.

5. **`crates/tunnel2tunnel-web/src/routes/live_ws.rs`** — replace `run_loop` and all three handler fns with one `live_connections_ws`, registered at `GET /api/live-connections/ws` in `crates/tunnel2tunnel-web/src/lib.rs`, holding per-connection mutable scope state:
   ```rust
   pub async fn live_connections_ws(AuthUser(user): AuthUser, State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
       ws.on_upgrade(move |socket| async move { run_loop(socket, state, user).await })
   }

   async fn run_loop(mut socket: WebSocket, state: AppState, user: User) {
       let mut scope = Scope::Mine;
       let mut rx = state.live_event_tx.subscribe();
       let mut interval = tokio::time::interval(FALLBACK_RESYNC_INTERVAL);
       // initial send, reason: None
       loop {
           tokio::select! {
               _ = interval.tick() => { /* rebuild(scope), send reason: None */ }
               recv = rx.recv() => match recv {
                   Ok(ev) => {
                       let relevant = match scope { Scope::Mine => ev.entity_ids().iter().any(|id| /* in user's owned set */), Scope::All => true };
                       if relevant { /* rebuild(scope), send reason: Some(ev) */ }
                   }
                   Err(Lagged(_)) => { /* rebuild(scope), send reason: None */ }
                   Err(Closed) => break,
               },
               msg = socket.recv() => match msg {
                   Some(Ok(Message::Text(t))) => {
                       if let Ok(cmd) = serde_json::from_str::<ClientCommand>(&t) {
                           if cmd.scope == Scope::All && !user.is_admin { /* ignore, log warn */ }
                           else { scope = cmd.scope; /* rebuild(scope), send reason: None */ }
                       }
                   }
                   None | Some(Err(_)) => break,
                   _ => {}
               }
           }
       }
   }
   ```
   `ClientCommand { cmd: "set_scope", scope: Scope }`, `Scope: Mine | All`. Envelope sent is `WithReason<Vec<EntityLiveSnapshot>>` (`{ data: [...], reason? }`) every time, regardless of scope. This is a straight replacement of the old generic `run_loop`/three-handler-fn setup — no other route exists to keep byte-identical anymore, so no `Option<...>`-gated generic parameter is needed.

### Frontend

6. **`frontend/src/stores/liveConnections.ts`** (new Pinia store) — owns the *one* app-wide connection to `/api/live-connections/ws`:
   - `snapshots: EntityLiveSnapshot[]`, `connected: boolean`, `scope: 'mine' | 'all'`
   - `connect()` / `disconnect()` — manual lifecycle, not component-mount-tied, plus a `send()` for the scope command. `composables/useLiveSocket.ts` doesn't fit as-is for two independent reasons: (a) it opens in `onMounted`/closes in `onUnmounted` — tied to a component's life, but this connection must outlive every component (survive route-keyed remounts); (b) it's receive-only (`onMessage` callback), no way to push a command frame back. Pull its reconnect/backoff engine out into a plain `createLiveSocket(path, onMessage)` helper returning `{ close(), send(data) }`, called directly by the store (not wrapped in a composable). Since every consumer (Dashboard, EntityDetail, Admin) moves onto this store in this pass, `useLiveSocket.ts` itself ends up with no remaining call site — delete it rather than leave it unused.
   - `setScope(next)`: sends `{"cmd":"set_scope","scope":next}` over the open socket; also updates local `scope` optimistically (server will confirm via the next resync).
   - On every message (`{ data: EntityLiveSnapshot[], reason? }`): always replace `snapshots.value` and rebuild lookup maps: `entityNameById`, `serviceNameByKey` (key `` `${entity_id}:${port}` ``), and `myEntityIds: Set<string>` (every snapshot with `mine === true`). Only if `reason` present: resolve names from the just-rebuilt maps, check the reason's entity id(s) against `myEntityIds` — **skip the toast entirely if none match** (covers the `all`-scope case: table updates for other people's entities must never toast). Otherwise decide "my" perspective for bridge events (is `client_entity_id` or `target_entity_id` mine) and call `useToast().show(text, level)`. Suggested copy/level, reusing `liveStatus.ts` framing:
     - `entity_online` → "`{name}` is online" / `success`
     - `entity_offline` → "`{name}` went offline" / `info`
     - `port_forwarding_started` → "`{service}` on `{name}` started forwarding" / `info`
     - `port_forwarding_stopped` → "`{service}` on `{name}` stopped forwarding" / `info`
     - `bridge_started` → "Connected to `{service}` on `{target}`" (mine = client) or "`{client}` connected to `{service}`" (mine = target) / `success`
     - `bridge_stopped` → mirrored, "Disconnected from…" / "`{client}` disconnected from…" / `info`

7. **`frontend/src/App.vue`** — `watch(() => authStore.user, user => user ? liveConnectionsStore.connect() : liveConnectionsStore.disconnect(), { immediate: true })`. `immediate: true` matters beyond the obvious login case: on a page reload while already logged in, `App.vue` remounts fresh with `user` starting `null` until the existing bootstrap `fetchMe()` resolves from the still-valid session cookie — the watcher (immediate or not) catches that null→user transition fine; `immediate` additionally covers the edge case where `user` is already populated by the time the watcher registers, so it doesn't depend on a change firing at all. Move `<ToastContainer />` here from `AppShell.vue` so toasts render across the route-keyed remount and on the login screen.

8. **`frontend/src/pages/DashboardPage.vue`** — drop its own socket call and local `rows`/`loading` refs. Read `snapshots.filter(s => s.mine)` from the store via `storeToRefs`, derive table rows with a computed `flatMap` (one row per service + one per subscription per entity, mirroring what `DashboardRow` used to be built as, just client-side now), `connected`-derived loading state.

9. **`frontend/src/pages/EntityDetailPage.vue`** — drop its own socket call for the live-connections part. Read `snapshots` from the store, `computed(() => snapshots.value.find(s => s.entity_id === entityId))` for this entity's `services`/`subscriptions`/`entity_online`/`entity_last_disconnected_at` — instant on navigation since the store is already warm.

10. **`frontend/src/pages/AdminLiveConnectionsPage.vue`** — drop its own socket call. `onMounted → store.setScope('all')`, `onUnmounted → store.setScope('mine')`. Render from `snapshots` (now including every user's entities while mounted), using the now-present `account`/admin-only fields for the columns that used to come from `LiveConnectionRow`.

11. **`frontend/src/api/entities.ts` / `admin.ts`** — remove now-dead `DashboardRow`/`LiveConnectionRow`/`EntityLiveConnectionsResponse`/`EntityLiveMessage` fetch-by-websocket-path types, replace with the single `EntityLiveSnapshot` TS type (reusing `ServiceLiveStatus`/`SubscriptionLiveStatus` as before).

## Verification

- `cargo build -p t2t` compiles.
- Manual: run backend + frontend per `CLAUDE.md`'s local-run instructions. Log in, open Dashboard, then in another terminal SSH-register a port (`-R`) and open a `-L` tunnel from a second entity:
  - Expect a toast the moment `tcpip_forward` runs (before any page polls anything) — confirms server-push, not client poll.
  - Expect a toast on bridge open (`-L` connects) and on bridge close (Ctrl-C the tunnel).
  - Navigate Dashboard → Entities → EntityDetail → back to Dashboard while a tunnel is open/closing — confirm the toast still fires (socket survived the route-keyed remount), only one `/api/live-connections/ws` upgrade happens total (check backend log / Network tab, not one per navigation), and EntityDetail's services/subscriptions render immediately on open (no loading flash).
  - As an admin, open the admin Live Connections page — confirm it shows all users' entities (not just admin's own), and switching away then back to Dashboard stops seeing other users' rows (scope reverted).
  - Confirm no toasts for another user's entities even while an admin's browser tab is in `all` scope.
  - Confirm `/api/entities/{id}/live-connections/ws` and `/api/admin/live-connections/ws` are both actually gone (404/no route) and nothing in the frontend still references them.

## Todos

- [x] Add LiveEvent enum + broadcast channel in tunnel2tunnel-ssh
- [x] Emit LiveEvent at the 8 SSH mutation sites + Drop cleanup
- [x] Wire live_event_tx through AppState + main.rs
- [x] Unify live_connections.rs into EntityLiveSnapshot + build_live_connections
- [x] Replace 3 live_ws routes with single live_connections_ws
- [x] Frontend: liveConnections Pinia store + createLiveSocket helper
- [x] Wire App.vue auth watcher + move ToastContainer
- [x] Migrate DashboardPage/EntityDetailPage/AdminLiveConnectionsPage to store
