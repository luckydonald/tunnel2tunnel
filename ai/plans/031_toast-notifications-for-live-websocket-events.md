# Toast notifications for live websocket events

## Context

Live status (connections, port forwarding, bridging) already streams to the frontend over websockets, but each socket just re-pushes a *full state snapshot* on every backend change (`useLiveSocket.ts` overwrites local refs wholesale — no discrete "this specific thing changed" event anywhere). The user wants toast popups for concrete events — connection up/down, port forwarding start/stop, and bridging (idle↔active) — driven by real server-side pushes at the exact moment the state changes (not client-side diffing of snapshots, not a `GET` disguised as a websocket). Toasts must show up regardless of which page/route is open, scoped to the logged-in user's own entities.

While designing this, a broader cleanup fell out naturally and is in scope for this pass: today there are three near-duplicate live-status websocket routes (`/api/entities/{id}/...`, `/api/me/...`, `/api/admin/...`). `build_my_live_connections` (the `me` route's builder) **already internally calls** `build_entity_live_connections` per owned entity and then discards the nested detail (`services[].subscribers[]`, `entity_online`) to flatten into `DashboardRow`. Widening `me`'s payload to keep that nested detail lets `/api/entities/{id}/...` be retired entirely — `EntityDetailPage.vue` becomes a client-side filter over data already synced by the one global store, making entity navigation instant (no new connection, no loading spinner). The admin route stays separate (different shape — `account`/`peer_ip` fields that must never leak to non-admins — and out of toast scope per user), but is generalized to use the same push mechanism for free.

Explicit decisions made during design (do not revisit without reason):
- **No server-side diff engine.** Every state transition already happens at a known, specific code location (SSH mutation site) where old/new state is directly in hand — emit the discrete event *there*, don't reconstruct it later by diffing two snapshots. Diffing would solve a problem that doesn't exist here.
- **One message = full snapshot + optional `reason`.** Not two message types (`snapshot` vs `event`) — every push already is the full current state; `reason` just names *why* this particular push happened. `reason: None` (initial connect / 20s fallback resync / lagged-resync) → state update only, no toast.
- **Toasts scoped to `me` only.** Admin gets the same mechanism (`reason` field) for free via the generalized loop, but no toast UI is wired to the admin page in this pass.

Existing infra confirmed reusable as-is: `frontend/src/composables/useToast.ts` (singleton `show(message, level, durationMs)`) + `frontend/src/components/ToastContainer.vue` (`Teleport to="body"`). Broadcast plumbing: payload-less `broadcast::Sender<()>` (`LiveUpdateTx`, `crates/tunnel2tunnel-ssh/src/lib.rs:71`) shared between the SSH server and web `AppState`, used today by all three routes' `run_loop` (`crates/tunnel2tunnel-web/src/routes/live_ws.rs:129-171`) as a bare "something changed, rebuild" wakeup.

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

3. **`crates/tunnel2tunnel-web/src/lib.rs`** — add `live_event_tx: LiveEventTx` to `AppState`. **`crates/t2t/src/main.rs`** — construct via `new_live_event_tx()` next to `new_live_update_tx()`, pass to both `start_ssh(...)` and the web `AppState`.

4. **`crates/tunnel2tunnel-web/src/routes/live_connections.rs`** — widen `build_my_live_connections` (`~394-428`) from returning flat `Vec<DashboardRow>` to `Vec<EntityLiveSnapshot>`, where `EntityLiveSnapshot` is today's per-entity nested shape plus identity:
   ```rust
   #[derive(Serialize)]
   pub struct EntityLiveSnapshot {
       pub entity_id: Uuid,
       pub entity_name: Option<String>,
       pub entity_online: bool,
       pub entity_last_disconnected_at: Option<OffsetDateTime>,
       pub services: Vec<ServiceLiveStatus>,       // already exists, includes subscribers[]
       pub subscriptions: Vec<SubscriptionLiveStatus>, // already exists
   }
   ```
   The function already computes all of this per owned entity via its existing internal call to `build_entity_live_connections` — this is "stop discarding computed detail before serializing," not new query logic. Delete the `DashboardRow`-flattening step (and the type itself, once nothing else needs it) — `DashboardPage.vue` will flatten client-side instead (step 8). Delete `/api/entities/{id}/live-connections/ws`, `entity_live_connections_ws`, and its route registration in `crates/tunnel2tunnel-web/src/lib.rs`; `build_entity_live_connections` stays (still called internally by `build_my_live_connections`), just loses its standalone HTTP-exposed caller.

5. **`crates/tunnel2tunnel-web/src/routes/live_ws.rs`** — generalize `run_loop` in place (not a parallel function), so the admin route stays byte-identical aside from opting in:
   ```rust
   async fn run_loop<T, F, Fut>(
       who: &str, mut socket: WebSocket, state: AppState, build: F,
       events: Option<(broadcast::Receiver<LiveEvent>, fn(&LiveEvent, &T) -> bool)>,
   )
   where T: Serialize, F: Fn(AppState) -> Fut, Fut: Future<Output = Result<T, WebError>>,
   ```
   `events: None` (admin route) → unchanged behavior, `T` serialized bare, only `live_update_tx`/interval drive resync.
   `events: Some((rx, filter))` (`me` route only) → serialized envelope gains an optional `reason`:
   ```rust
   #[derive(Serialize)]
   struct WithReason<T> { #[serde(flatten)] data: T, #[serde(skip_serializing_if = "Option::is_none")] reason: Option<LiveEvent> }
   ```
   `my_live_connections_ws` stops subscribing to the bare `live_update_tx` entirely — its `events` receiver (`state.live_event_tx.subscribe()`) is both wake-signal and payload — with a `filter` closure checking `ev.entity_ids()` against `rows.iter().map(|r| r.entity_id)`:
   - initial connect / interval tick / `Lagged` → rebuild + send `reason: None`.
   - `rx.recv() == Ok(ev)` and `filter(&ev, &rebuilt_rows)` true → rebuild + send `reason: Some(ev)`.
   - `rx.recv() == Ok(ev)` but irrelevant to this user → skip sending anything.
   Admin route also switches to `events: Some((state.live_event_tx.subscribe(), |_, _| true))` — same mechanism, always-pass filter, `reason` riding along for free even though nothing reads it yet on the frontend.

### Frontend

6. **`frontend/src/stores/liveConnections.ts`** (new Pinia store) — owns the *one* app-wide connection to `/api/me/live-connections/ws`:
   - `snapshots: EntityLiveSnapshot[]`, `connected: boolean`
   - `connect()` / `disconnect()` — manual lifecycle, not component-mount-tied. Extract the reconnect/backoff logic out of `composables/useLiveSocket.ts` into a plain `createLiveSocket(path, onMessage)` helper returning `{ close() }`; `useLiveSocket` (still used by `AdminLiveConnectionsPage.vue`, unchanged) wraps it in `onMounted`/`onUnmounted` as before, the store calls it directly.
   - On every message (`{ ...EntityLiveSnapshot[] fields flattened per entry, reason? }` — actual shape: `{ data: EntityLiveSnapshot[], reason? }` per `WithReason<Vec<EntityLiveSnapshot>>`): always replace `snapshots.value` and rebuild lookup maps: `entityNameById: Map<string,string>`, `serviceNameByKey: Map<string,string>` (key `` `${entity_id}:${port}` ``, built from each snapshot's `services`/`subscriptions`), `myEntityIds: Set<string>` (every `snapshot.entity_id`). Only if `reason` is present: resolve names from the just-rebuilt maps (fall back to raw id/port if unknown), decide "my" perspective for bridge events (is `client_entity_id` or `target_entity_id` mine), call `useToast().show(text, level)`. Suggested copy/level, reusing `liveStatus.ts` framing:
     - `entity_online` → "`{name}` is online" / `success`
     - `entity_offline` → "`{name}` went offline" / `info`
     - `port_forwarding_started` → "`{service}` on `{name}` started forwarding" / `info`
     - `port_forwarding_stopped` → "`{service}` on `{name}` stopped forwarding" / `info`
     - `bridge_started` → "Connected to `{service}` on `{target}`" (mine = client) or "`{client}` connected to `{service}`" (mine = target) / `success`
     - `bridge_stopped` → mirrored, "Disconnected from…" / "`{client}` disconnected from…" / `info`

7. **`frontend/src/App.vue`** — watch `useAuthStore().user` and call `liveConnectionsStore.connect()` when non-null, `disconnect()` when null. Move `<ToastContainer />` here from `AppShell.vue` so toasts render across the route-keyed remount and on the login screen.

8. **`frontend/src/pages/DashboardPage.vue`** — drop its own `useLiveSocket(...)` call and local `rows`/`loading` refs. Read `snapshots` from the store via `storeToRefs`, derive its table rows with a computed `flatMap` (one row per service + one per subscription per entity, mirroring what `DashboardRow` used to be built as, just client-side now) and a `connected`-derived loading state.

9. **`frontend/src/pages/EntityDetailPage.vue`** — drop its own `useLiveSocket<EntityLiveMessage>(...)` call for the live-connections part. Read `snapshots` from the store, `computed(() => snapshots.value.find(s => s.entity_id === entityId))` for this entity's `services`/`subscriptions`/`entity_online`/`entity_last_disconnected_at` — same shape it already renders, now instant on navigation since the store is already warm.

10. **`frontend/src/api/entities.ts`** — remove the now-dead `EntityLiveConnectionsResponse`/`EntityLiveMessage` fetch-by-websocket-path usage if no longer referenced elsewhere (keep the types themselves if `ServiceLiveStatus`/`SubscriptionLiveStatus` are reused for the new `EntityLiveSnapshot` TS type — likely just move/rename).

`AdminLiveConnectionsPage.vue` is untouched — same socket, same shape, `reason` field arrives but nothing reads it yet.

## Verification

- `cargo build -p t2t` compiles.
- Manual: run backend + frontend per `CLAUDE.md`'s local-run instructions. Log in, open Dashboard, then in another terminal SSH-register a port (`-R`) and open a `-L` tunnel from a second entity:
  - Expect a toast the moment `tcpip_forward` runs (before any page polls anything) — confirms server-push, not client poll.
  - Expect a toast on bridge open (`-L` connects) and on bridge close (Ctrl-C the tunnel).
  - Navigate Dashboard → Entities → EntityDetail → back to Dashboard while a tunnel is open/closing — confirm the toast still fires (socket survived the route-keyed remount), only one `/api/me/live-connections/ws` upgrade happens total (check backend log / Network tab, not one per navigation), and EntityDetail's services/subscriptions render immediately on open (no loading flash) since data was already synced.
  - Confirm no toasts for another user's entities (second, unrelated user logged in in a private window while the first user's tunnel is active).
  - Confirm `/api/entities/{id}/live-connections/ws` route is actually gone (404/no route) and nothing in the frontend still references it.
