Task complete. Findings below.

**A) Frontend**
- Status dot component: `frontend/src/components/StatusDot.vue` — takes `live: boolean`, `remote_status?: RemoteStatus` (`gray|orange|green`); emoji (🟢/⚪) is the dot, `ring-*` CSS class is the ring. Colors/labels centralized in `frontend/src/liveStatus.ts` (`dotEmoji`, `dotLabel`, `ringLabel`).
- **No polling exists today** — contrary to assumption, all three consumers fetch once `onMounted` and only refresh on manual button click or after a mutating action (subscribe/unsubscribe): `DashboardPage.vue:84`, `EntityDetailPage.vue:127` (`loadLiveConnections`), `AdminLiveConnectionsPage.vue:46` (`load`, has a "Refresh" button `AdminLiveConnectionsPage.vue:53`).
- Endpoints: `entitiesApi.getLiveConnections(entityId)` → `GET /api/entities/{id}/live-connections`; `adminApi.listLiveConnections()` → admin live-connections list endpoint.
- Dot vs ring split (per commit 7d61917, documented in `liveStatus.ts:1-14` and backend module doc `live_connections.rs:1-18`): `live` (dot) = is traffic flowing now; `remote_status` (ring) = counterpart's SSH/port state, tri-state gray/orange/green, only set for subscriber-side rows (service rows always `null`).
- Admin Live Connections: `frontend/src/pages/AdminLiveConnectionsPage.vue`, route likely `admin-live-connections` (referenced `DashboardPage.vue:95`).
- No WebSocket client anywhere in frontend; no ws lib in `package.json`.

**B) Backend**
- No `WebSocketUpgrade`/axum ws support anywhere in `crates/` — zero matches. Nothing to build on; a ws endpoint + upgrade handler is new work.
- Source of truth for live state lives in `tunnel2tunnel-ssh/src/lib.rs`:
  - `ServerSlots = Arc<Mutex<HashMap<(Uuid, u32), (Handle, String)>>>` — "port forwarded" state (`lib.rs:43`).
  - `ActiveTunnels = Arc<Mutex<HashMap<Uuid, ActiveTunnelInfo>>>` — live bridged connections (`lib.rs:64`, struct at `lib.rs:52`).
  - Both are already shared into `AppState` (web crate) and locked/snapshotted per-request in `crates/tunnel2tunnel-web/src/routes/live_connections.rs:121-129` and `233-241`.
- No existing pub/sub/broadcast channel — state is polled by locking the maps fresh on each HTTP request. A `tokio::sync::broadcast` (or similar) channel would need to be added, fired wherever `ServerSlots`/`ActiveTunnels` mutate (tcpip_forward, channel_open_direct_tcpip, disconnects) in `tunnel2tunnel-ssh/src/lib.rs`.
- `connection_logs` (migration 004) is used only for `ConnectionLog::entity_statuses` (online/offline history, `live_connections.rs:174`), not for the live dot/ring itself — that's pure in-memory state, DB is only consulted for "owner online" history.

**C) EntityDetailPage debug data**
- Already-loaded reactive state available client-side to serialize: `entity` (`EntityDetail` — includes ports, ssh_keys, is_server/is_client, online, last_disconnected_at), `serviceLiveStatus`, `subscriptionLiveStatus`, `subscribableOwners`, `accessRules`, `incomingGrants`, `connLogs` (lazy, only if user opened that section), `ownSubscriptionRows`. All in `EntityDetailPage.vue`.
- No existing `navigator.clipboard` usage anywhere in repo — this would be new. No `<kbd>` usage found either — new styling needed.

**Gaps to design for**: ws upgrade route + auth (session cookie) in web crate, broadcast wiring in ssh crate at every state-mutation site, frontend ws composable/store to replace `onMounted` fetches + manual refresh buttons, and a debug-JSON assembler + clipboard button + `<kbd>` styling on `EntityDetailPage.vue`.