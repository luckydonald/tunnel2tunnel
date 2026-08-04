# Toast notifications for live websocket events

## Context

Live status (connections, port forwarding, bridging) already streams to the frontend over websockets, but each socket just re-pushes a *full state snapshot* on every backend change (`useLiveSocket.ts` overwrites local refs wholesale — no discrete "this specific thing changed" event, no diffing anywhere, client or server). The user wants toast popups for concrete events — connection up/down, port forwarding start/stop, and bridging (idle↔active) — driven by real server-side pushes at the moment the state actually changes (not the client polling/diffing snapshots, and not disguising a `GET` as a websocket). Toasts must show up regardless of which page/route is open, but only for the logged-in user's own entities (no admin-wide feed).

Research findings (from two Explore passes):
- Toast infra already exists and needs no changes: `frontend/src/composables/useToast.ts` (singleton `show(message, level, durationMs)`) + `frontend/src/components/ToastContainer.vue` (`Teleport to="body"`).
- Backend push mechanism: a payload-less `broadcast::Sender<()>` (`LiveUpdateTx`, `crates/tunnel2tunnel-ssh/src/lib.rs:71`) shared between the SSH server and web `AppState`. Every relevant SSH-side mutation (`crates/tunnel2tunnel-ssh/src/lib.rs`: auth success ~459, `tcpip_forward` ~1053, `cancel_tcpip_forward` ~1084, `channel_open_direct_tcpip` fast path ~1307 / slow path ~1430, `channel_close` ~1508-1511, and three `Drop` cleanup tasks ~1550-1600) already calls `live_update_tx.send(())` right where old/new state is known — these are exactly the points to also emit a discrete event.
- `/api/me/live-connections/ws` (`crates/tunnel2tunnel-web/src/routes/live_ws.rs:98-110`, backed by `build_my_live_connections`, `live_connections.rs:394-428`) is already scoped to `Entity::list_for_user(user_id)` — the correct, already-existing per-user aggregation point. No admin/entity-detail sockets need touching.
- Frontend today opens this socket locally inside `DashboardPage.vue`'s `onMounted`, wrapped by `<AppShell>` per-page — and `App.vue` remounts the routed component (`<RouterView :key="route.path" />`) on every navigation, so anything living inside a page component (including `AppShell`) is destroyed/recreated on route change. That's incompatible with "toasts independent of route," so the socket needs to move to a place that survives navigation: `App.vue` itself (never keyed/remounted) via a Pinia store.

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
   Deliberately minimal payloads (ids + port only, no names) — the frontend already receives entity/service names via the snapshot messages on the same socket and can enrich from a local lookup instead of the SSH hot path doing extra DB work.

2. Add a `live_event_tx: LiveEventTx` field to `T2tHandler` (threaded through its constructor next to `live_update_tx`), and emit one `let _ = self.live_event_tx.send(LiveEvent::…)` beside each existing `live_update_tx.send(())` call listed above, using values already in scope:
   - auth success → `EntityOnline`
   - `tcpip_forward` → `PortForwardingStarted`
   - `cancel_tcpip_forward` → `PortForwardingStopped`
   - `channel_open_direct_tcpip` (both fast path and the spawned slow-path task — clone `live_event_tx` into the spawn like `live_update_tx` already is) → `BridgeStarted`
   - `channel_close` → capture the `Option<ActiveTunnelInfo>` returned by `active_tunnels.lock().await.remove(&bridge_id)` (currently discarded) to get `client_entity_id`/`target_entity_id`/`proxy_port` → `BridgeStopped`
   - `Drop`'s three cleanup tasks → `EntityOffline` (entity-online task, already has `entity_id`), and `BridgeStopped` per removed id in the active-tunnels cleanup task (capture each `ActiveTunnelInfo` before removing, same as `channel_close`). The `server_slots` cleanup task removes by entity id via `retain` — capture the removed `(entity_id, port)` keys before retaining to emit `PortForwardingStopped` per port.

3. **`crates/tunnel2tunnel-web/src/lib.rs`** — add `live_event_tx: LiveEventTx` to `AppState`; **`crates/t2t/src/main.rs`** — construct it via `new_live_event_tx()` next to `new_live_update_tx()` and pass to both `start_ssh(...)` and the web `AppState`.

4. **`crates/tunnel2tunnel-web/src/routes/live_ws.rs`** — new dedicated loop for `my_live_connections_ws` only (leave `run_loop`/the entity and admin routes untouched — they keep the old bare-array format):
   ```rust
   #[derive(Serialize)]
   #[serde(tag = "type", rename_all = "snake_case")]
   enum MyLiveMessage {
       Snapshot { rows: Vec<DashboardRow> },
       Event { #[serde(flatten)] event: LiveEvent },
   }
   ```
   Loop: subscribe to both `state.live_update_tx` and `state.live_event_tx`; keep a `HashSet<Uuid>` of the user's own entity ids, refreshed every time a snapshot is (re)built (`rows.iter().map(|r| r.entity_id).collect()`); on a snapshot trigger (event-driven wakeup, the existing 20s fallback interval, or initial connect) rebuild + send `Snapshot`; on an event-channel recv, send `Event` only if `event.entity_ids()` intersects the id set. `Lagged` on the event receiver is just dropped (a missed toast isn't correctness-critical, unlike a missed snapshot); `Lagged`/tick on the snapshot receiver behaves exactly as `run_loop` does today.

### Frontend

5. **`frontend/src/stores/liveConnections.ts`** (new Pinia store) — owns the *one* app-wide connection to `/api/me/live-connections/ws`:
   - `rows: DashboardRow[]`, `connected: boolean`
   - `connect()` / `disconnect()` — manual lifecycle (not tied to component mount). Reuse the reconnect/backoff logic in `composables/useLiveSocket.ts` by extracting it into a plain `createLiveSocket(path, onMessage)` helper returning `{ close() }` that `useLiveSocket` (still used by `EntityDetailPage.vue`/`AdminLiveConnectionsPage.vue`, unchanged) wraps in `onMounted`/`onUnmounted`, and the store calls directly.
   - On `{type: 'snapshot', rows}`: replace `rows.value`, and update two lookup maps kept in the store: `entityNameById: Map<string,string>` and `serviceNameByKey: Map<string,string>` (key `` `${entity_id}:${port}` ``) from the row data, plus `myEntityIds: Set<string>` (every `row.entity_id` — `DashboardRow.entity_id` is always *my* entity per `build_my_live_connections`).
   - On `{type: 'event', kind, ...}`: resolve names from the maps (fall back to raw id/port if unknown), decide the "my" perspective for bridge events (is `client_entity_id` or `target_entity_id` mine — phrase "connected to X" vs "X connected to your Y"), then call `useToast().show(text, level)`. Suggested copy/level, reusing `liveStatus.ts` framing where possible:
     - `entity_online` → "`{name}` is online" / `success`
     - `entity_offline` → "`{name}` went offline" / `info`
     - `port_forwarding_started` → "`{service}` on `{name}` started forwarding" / `info`
     - `port_forwarding_stopped` → "`{service}` on `{name}` stopped forwarding" / `info`
     - `bridge_started` → "Connected to `{service}` on `{target}`" (mine = client) or "`{client}` connected to `{service}`" (mine = target) / `success`
     - `bridge_stopped` → mirrored, "Disconnected from…" / "`{client}` disconnected from…" / `info`

6. **`frontend/src/App.vue`** — watch `useAuthStore().user` (already a reactive ref) and call `liveConnectionsStore.connect()` when it becomes non-null, `disconnect()` when it becomes null; also move `<ToastContainer />` here from `AppShell.vue` (small change) so toasts render even during the route-keyed remount gap between pages, and on the login screen too.

7. **`frontend/src/pages/DashboardPage.vue`** — drop its own `useLiveSocket<DashboardRow[]>(...)` call and local `rows`/`loading` refs; read `rows` (and a `connected`-derived loading state) from the new store via `storeToRefs`. This avoids a second redundant websocket connection to the same route.

`EntityDetailPage.vue` and `AdminLiveConnectionsPage.vue` are untouched — their sockets/pages keep working exactly as today.

## Verification

- `cargo build -p t2t` compiles; `cargo test` if any existing SSH/web tests cover `live_update_tx` usage still pass.
- Manual: run backend + frontend per `CLAUDE.md`'s local-run instructions. Log in, open Dashboard, then in another terminal SSH-register a port (`-R`) and open a `-L` tunnel from a second entity:
  - Expect a toast the moment `tcpip_forward` runs (before any page polls anything) — confirms server-push, not client poll.
  - Expect a toast on bridge open (`-L` connects) and on bridge close (Ctrl-C the tunnel).
  - Navigate between Dashboard → Entities → back to Dashboard while a tunnel is open/closing — confirm the toast still fires (proves the socket survived the route-keyed remount) and only one socket connects (check Network tab / backend log for a single `/api/me/live-connections/ws` upgrade, not one per navigation).
  - Confirm no toasts appear for another user's entities (log in as a second, unrelated user in a private window while the first user's tunnel is active).
