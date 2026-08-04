Design implementation plan for tunnel2tunnel repo. Context gathered from research:

BUG 1: Port dot stays gray even though client (entity) shows online. Root cause: two independent backend signals conflated in user's mental model but actually separate already — `entity.online` (crates/tunnel2tunnel-core/src/models/connection_log.rs:330-370, wired via crates/tunnel2tunnel-web/src/routes/entities.rs:96-128) flips true on SSH auth success alone (connection_logs row with ended_at IS NULL). Port-level green dot (crates/tunnel2tunnel-web/src/routes/live_connections.rs:111-147 for "my services" via `live_slots`/`server_slots`, and status_for_subscription() lines 276-286 for "my subscriptions" via `active_tunnels`) requires a further channel-level SSH event (tcpip_forward at crates/tunnel2tunnel-ssh/src/lib.rs:976, or direct-tcpip channel open at :1273/:1394). So a client can be authenticated (online=true) but its port forward/channel not yet established (still gray) — this is legitimate/expected per current model, not a stale-query bug, BUT the frontend currently gives no indication of *why* it's gray / doesn't distinguish "client offline entirely" vs "client online but this specific port/channel not yet up".

FEATURE 2 (user's actual ask, related): status icon should show TWO independent visual channels instead of one collapsed 3-way enum:
- Outer ring: orange when "remote is not connected" (need to determine what "remote" means precisely — likely the OTHER side of the tunnel, i.e. for a subscription row, whether the entity providing the service/port is online; for a service row, whether the subscribing client is connected — needs clarification, but working theory: ring reflects entity.online-style presence of the counterpart)
- Main dot color: still reflects the client's own connected/disconnected state as today (green=live channel, gray=not live)

Currently frontend/src/components/StatusDot.vue (~23 lines) takes single `LiveStatus` prop ('green'|'gray'|'orange') defined in frontend/src/liveStatus.ts:6-19, fed directly from backend's single-signal `status` field. Consumers: frontend/src/pages/DashboardPage.vue:111, EntityDetailPage.vue:511, AdminLiveConnectionsPage.vue:89. Backend route crates/tunnel2tunnel-web/src/routes/live_connections.rs computes this collapsed status server-side today (lines ~62-64 comment says orange currently unused/reserved).

TEST REQUIREMENT: Add API/state e2e test coverage for connection scenarios: client connected + port active (green), client connected + port not yet active/gray, client fully disconnected, and the new "remote not connected" orange-ring scenario. Existing Rust e2e suite: crates/t2t/tests/tunnel_e2e.rs, with helpers wait_for_port_config (line 296), wait_for_active_tunnel_entry (line 321), scenario tests two_ssh_connections_tunnel_through_rendezvous (line 352) and same_connection_online_target_not_blocked_by_offline_target_timeout (line 837). live_connections.rs's status_for_subscription() (line 276) currently has NO dedicated unit tests. Frontend spec files: frontend/src/components/StatusDot.spec.ts (21 lines, 3 cases) and frontend/src/pages/DashboardPage.spec.ts.

Please read the actual current contents of these key files to ground the plan precisely:
- crates/tunnel2tunnel-web/src/routes/live_connections.rs (full file)
- frontend/src/components/StatusDot.vue
- frontend/src/liveStatus.ts
- crates/tunnel2tunnel-core/src/models/connection_log.rs (entity_status/entity_statuses functions area)
- frontend/src/pages/DashboardPage.vue and EntityDetailPage.vue and AdminLiveConnectionsPage.vue (the relevant status-consuming sections)
- crates/t2t/tests/tunnel_e2e.rs (structure, existing helpers, the two named tests above)
- frontend/src/components/StatusDot.spec.ts

Then design a concrete plan:
1. Exact API/type changes needed: what new field(s) does the backend need to add to the live-connections response (e.g. separate `port_live: bool` and `remote_online: bool` fields, or similar) to let the frontend independently render main-dot color and ring color. Decide the precise semantic of "remote" for both the "my services" and "my subscriptions" list variants (what counterpart entity's online status applies in each case).
2. Exact StatusDot.vue prop/rendering changes (two independent props instead of one enum; CSS for an outer ring in orange).
3. Exact changes needed in DashboardPage.vue / EntityDetailPage.vue / AdminLiveConnectionsPage.vue to pass the new fields through.
4. Backend unit tests to add for status_for_subscription()-equivalent logic (all branches: online+live, online+not-live, offline).
5. New/extended Rust e2e scenarios in tunnel_e2e.rs covering: client connected+port active, client connected+port gray, client disconnected, remote-not-connected — reusing existing helpers where possible.
6. New/extended frontend spec cases in StatusDot.spec.ts (and DashboardPage.spec.ts if warranted) for the two-channel rendering.
7. Migration/DB changes if any are actually required (likely none — this seems purely a query/API/frontend composition change, confirm this).

Return a concrete, file-by-file implementation plan with reasoning about the "remote" semantics decision, ordered by dependency (backend first, then frontend, then tests). Flag any open design questions that need user clarification (e.g., exact orange-ring semantics, whether existing 'orange' status value in LiveStatus is repurposed or removed).