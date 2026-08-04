# Realtime live-status via WebSocket + EntityDetail debug-data button

## Context

Status dots (service/subscription rings on `EntityDetailPage`, rows on `DashboardPage`, admin `AdminLiveConnectionsPage`) are currently one-shot HTTP fetches (`onMounted` + manual "Refresh" buttons) over `GET /api/entities/{id}/live-connections` and `GET /api/admin/live-connections`. `DashboardPage` additionally does an N+1 fetch (one `getLiveConnections` call per owned entity). None of this is push-based — a remote's state change (port forwarded, subscriber connects/disconnects, entity comes online) is invisible until the user reloads or clicks Refresh. Goal: make all three views push-updated over WebSocket, sourced from the same in-memory `ServerSlots`/`ActiveTunnels` state (`tunnel2tunnel-ssh/src/lib.rs`) already shared into `AppState`, notified the instant that state changes.

Also add a "copy debug data" button on `EntityDetailPage` that serializes everything currently known about that entity/its connections into JSON on the clipboard, for bug reports.

## Backend

### 1. Enable axum's `ws` feature
Root `Cargo.toml:24` currently has no `features` array for axum (confirmed: `ws` is not a default feature). Add `features = ["ws"]` to the workspace `axum` dependency.

### 2. Shared change-notifier
Add a `LiveUpdateTx = tokio::sync::broadcast::Sender<()>` type + `pub fn new_live_update_tx() -> LiveUpdateTx` (capacity 16) in `tunnel2tunnel-ssh/src/lib.rs`, next to `new_server_slots`/`new_active_tunnels`. Construct one instance in `crates/t2t/src/main.rs` alongside the existing `server_slots`/`active_tunnels`, pass into both `tunnel2tunnel_ssh::start(...)` and `tunnel2tunnel_web::AppState`.

Fire `let _ = live_update_tx.send(());` (ignore `SendError` — no receivers is fine) at every state-mutation site in `tunnel2tunnel-ssh/src/lib.rs`:
- `tcpip_forward` — after `server_slots.lock().await.insert(...)` (~line 1030)
- `cancel_tcpip_forward` — after `server_slots.lock().await.remove(...)` (~line 1060)
- `channel_open_direct_tcpip` fast path — after `active_tunnels.lock().await.insert(...)` (~line 1273)
- `channel_open_direct_tcpip` slow path (spawned task) — after its `active_tunnels.lock().await.insert(...)` (~line 1394)
- `channel_close` — after removing from `active_tunnels` (~line 1484)
- `Drop` — after the `server_slots.retain(...)` cleanup (~line 1530) and after the `active_tunnels` cleanup (~line 1544)
- `log_auth_success` (~line 412) and the `Drop` block that calls `ConnectionLog::set_ended` (~line 1558) — these are the online/offline transitions (`ConnectionLog::entity_statuses` is what `entities.rs`'s `online`/`last_disconnected_at` and `live_connections.rs`'s `remote_status` both read)

This won't be perfectly exhaustive for every DB-side edit that could affect a ring color (e.g. someone disabling a `port_subscriptions` row via the HTTP API) — cover those with a periodic fallback tick in the WS handlers below (interval, ~20s) rather than wiring notifier calls into every HTTP mutation route; the in-memory-state transitions above are the ones that need to feel instant, HTTP-driven config edits reflecting within ~20s is fine.

### 3. Refactor snapshot builders (`tunnel2tunnel-web/src/routes/live_connections.rs`)
Extract the bodies of `list_entity_live_connections` and `list_admin_live_connections` into reusable async fns:
- `pub async fn build_entity_live_connections(state: &AppState, entity_id: Uuid) -> Result<EntityLiveConnectionsResponse, WebError>`
- `pub async fn build_admin_live_connections(state: &AppState) -> Result<Vec<LiveConnectionRow>, WebError>`

Existing HTTP handlers become thin wrappers (auth check + call + `Json(...)`) — no behavior change, existing tests keep passing.

Add one more: `pub async fn build_entity_status(state: &AppState, entity_id: Uuid) -> Result<(bool, Option<OffsetDateTime>), WebError>` — reuse whatever `entities.rs:251`'s `ConnectionLog::entity_statuses` call does (single-entity form), so the WS payload can carry live `online`/`last_disconnected_at` too (today only loaded once via `GET /api/entities/{id}`).

Add an aggregate builder for the dashboard (replaces its N+1 client-side fetch):
- `pub async fn build_my_live_connections(state: &AppState, user_id: Uuid) -> Result<Vec<DashboardRow>, WebError>` — loop the user's own entities (reuse whatever `entities.rs` uses to list a user's entities) and flatten each into rows, mirroring the flatten logic currently in `DashboardPage.vue`'s `load()` (service rows + subscription rows, same shape).

### 4. New WS routes (new file `tunnel2tunnel-web/src/routes/live_ws.rs`, wired in `lib.rs`)
- `GET /api/entities/{id}/live-connections/ws` — `AuthUser` + `require_owner`, same auth as the existing HTTP route.
- `GET /api/me/live-connections/ws` — `AuthUser`, backs the dashboard.
- `GET /api/admin/live-connections/ws` — `AdminUser`.

Each handler: `WebSocketUpgrade` (auth extractors run first — they only need request parts, `WebSocketUpgrade` consumes the rest), on upgrade spawn a loop that:
1. Sends one snapshot immediately (via the relevant builder(s) — for the per-entity route, bundle `build_entity_status` + `build_entity_live_connections` into one JSON message).
2. `tokio::select!`s a `live_update_tx.subscribe()` receiver against a `tokio::time::interval(20s)` fallback tick; on either firing (or `RecvError::Lagged`, treated the same as a fire — just resync), rebuilds and sends the snapshot again.
3. Exits the loop when the client-side send fails (socket closed).

### Tests
Extend `live_connections.rs`'s existing `#[cfg(test)]` module only if the refactor changes any logic (it shouldn't — pure extraction). No new backend integration test is required for the WS wiring itself given the size of this change, but do add one test asserting `live_update_tx.send(())` is actually reached from `tcpip_forward`/`channel_open_direct_tcpip`/`channel_close` (a receiver subscribed before the call observes exactly one notification) — cheap and catches a forgotten call site regressing silently.

## Frontend

### 5. `useLiveSocket` composable (new `frontend/src/composables/useLiveSocket.ts`)
Generic: `useLiveSocket<T>(path: string, onMessage: (data: T) => void): void`. Builds the URL from `location` (`(location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + path`), connects `onMounted`, `JSON.parse`s each message into `onMessage`, reconnects with capped exponential backoff (1s → 2s → 4s → … → 15s) on close/error, and closes the socket `onUnmounted`. Cookies ride along automatically on same-origin WS handshakes, so the existing session-cookie auth just works.

### 6. `EntityDetailPage.vue`
Replace `loadLiveConnections()` + its call sites (`load()`, `handleSubscribe`, `handleUnsubscribe`) with `useLiveSocket(`/api/entities/${entityId}/live-connections/ws`, data => { serviceLiveStatus.value = data.services; subscriptionLiveStatus.value = data.subscriptions; if (entity.value) { entity.value.online = data.entity.online; entity.value.last_disconnected_at = data.entity.last_disconnected_at } })`. Drops the need to manually re-fetch after subscribe/unsubscribe — the server-side notifier fires on those DB changes' next connection event, but to avoid a visible lag on the action the user just took, keep an optimistic manual `loadLiveConnections`-equivalent one-off refetch right after subscribe/unsubscribe (call the existing HTTP `getLiveConnections` once, since subscription enable/disable isn't one of the notifier's wired sites).

### 7. `DashboardPage.vue`
Replace the whole N+1 `load()` with `useLiveSocket('/api/me/live-connections/ws', data => { rows.value = data; loading.value = false })`.

### 8. `AdminLiveConnectionsPage.vue`
Replace `load()`/`onMounted(load)` with `useLiveSocket('/api/admin/live-connections/ws', data => { rows.value = data; loading.value = false })`. Drop the manual "Refresh" button (push makes it redundant).

### 9. Debug-data button (`EntityDetailPage.vue`)
Add a button in `.page-header` next to "Delete entity", styled as a small monospace/keyboard-key affordance (`<kbd>`-like pill), labeled "Copy debug data". On click:
- Fetch fresh connection logs (`adminApi.listConnectionLogs(entityId)`) regardless of whether the Connection Log section was ever opened, so the snapshot isn't stale.
- Assemble `{ generated_at: new Date().toISOString(), entity, live: { services: serviceLiveStatus, subscriptions: subscriptionLiveStatus }, access_rules: accessRules.length ? accessRules : await friendsApi.listAccess(entityId), incoming_grants: incomingGrants.length ? incomingGrants : await friendsApi.listIncomingAccess(entityId), subscribable_owners: subscribableOwners, recent_connection_logs: <freshly fetched> }`.
- `navigator.clipboard.writeText(JSON.stringify(obj, null, 2))`, wrapped in try/catch (clipboard permission can fail) → `toast('Debug data copied')` / `toast('Copy failed: ...')` via the existing `useToast` composable (no `navigator.clipboard` usage exists anywhere yet — this is new).

## Verification

- `cargo build` (workspace) — confirms the `ws` feature compiles and new routes type-check.
- `cargo test -p tunnel2tunnel-ssh -p tunnel2tunnel-web` — existing + new notifier test.
- `cd frontend && npm run type-check && npm run build`.
- Manual: run the stack per CLAUDE.md's "Running locally" section, open `EntityDetailPage` for a server entity and an `ssh -R` client in a terminal — toggle the forward (Ctrl-C the ssh client, restart it) and confirm the dot/ring update within ~1s with no page refresh. Do the same for `AdminLiveConnectionsPage` and `DashboardPage` in a second browser tab. Click "Copy debug data", paste the clipboard somewhere and confirm it's valid JSON containing the expected fields.
