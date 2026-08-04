# Decouple port/client status: dot = own liveness, ring = remote's tri-state

## Context

Bug report: connect client with a tunnel matching the webui port command → client shows online, but the port dot stays gray with no indication that the client actually is connected, just not yet routable. Root cause: today's single 3-way `status: green|gray|orange` field collapses two genuinely independent signals (own bridge liveness vs. remote SSH/port-forward state) into one enum, and `orange` is currently unused for owned services and doesn't consult `entity.online` at all — so "client authenticated but hasn't forwarded this port yet" is indistinguishable from "client fully offline." The user wants the status icon split into two independent visual channels: an inner dot for **my own** connection liveness, and an outer ring for **the remote counterpart's** status. Confirmed semantics (from user):

- **Inner dot** (my own side): gray = not connected/live, green = connected/live. For service (owner) rows this aggregates over all subscribers of that port: green if ≥1 subscriber currently has a live bridge, gray otherwise. No orange on the dot, ever.
- **Ring** (remote/counterpart side) — subscription rows only:
  - gray = remote entity not SSH-connected at all
  - orange = remote entity SSH-connected, but hasn't provided (forwarded) this port yet
  - green = remote entity SSH-connected and this port is forwarded/routable
- Service (owner) rows never show a ring (no single counterpart — 0..N subscribers; the existing subscriber list already shows who's connected).
- Wire format: replace the old `status: string` enum cleanly with the new fields (no back-compat shim needed — internal app, no external consumers).

## Backend changes

### `crates/tunnel2tunnel-core/src/models/connection_log.rs`
No changes — `entity_status`/`entity_statuses` (~line 330-372) already give `(online: bool, last_disconnected_at)` per entity from `connection_logs`, exactly what the ring's "remote SSH-connected" bit needs. Use the batch `entity_statuses` to avoid N+1 queries.

### `crates/tunnel2tunnel-web/src/routes/live_connections.rs`
- Replace `status: &'static str` on `ServiceLiveStatus`, `SubscriptionLiveStatus`, and admin `LiveConnectionRow` with:
  ```rust
  pub live: bool,                       // dot
  pub remote_status: Option<RemoteStatus>, // ring; None for service rows (never rendered)
  ```
  where `RemoteStatus` is a small serde enum `"gray" | "orange" | "green"`.
- `list_entity_live_connections`:
  - **services**: `live = subscribers.iter().any(|s| active_tunnels has a bridge for (this port, s.entity_id))` — aggregate over current subscribers instead of today's single `live_slots.contains(...)` port-forward check. `remote_status = None` always.
  - **subscriptions**: `live = has_active_bridge` (own channel open, unchanged from today's bridge check). `remote_status`: batch-fetch owner entity online flags via `entity_statuses`, then per row: `None`/gray if owner offline, `orange` if owner online but `!live_slots.contains(&(owner_entity_id, proxy_port))`, `green` if owner online and that slot is live. Gate all of this by `enabled` first — disabled subscription keeps `live=false, remote_status=gray` (matches today's "disabled → gray" behavior).
- `list_admin_live_connections`: same two-field change for `LiveConnectionRow`; batch the entity_statuses lookup once across all subscription-role rows in the response, not per row.
- Replace `status_for_subscription` with two small pure functions taking the same inputs (`enabled`, `has_active_bridge`, `owner_online`, `owner_port_live`) and returning `live: bool` / `remote_status: RemoteStatus` respectively — these are what get unit tests.
- Update the module doc comment (top of file) to describe the two-channel model.

## Frontend changes

### `frontend/src/liveStatus.ts`
Drop the `LiveStatus` string union used for wire data. Add a `RemoteStatus = 'gray' | 'orange' | 'green'` type (ring) and keep `live: boolean` (dot) separate. Replace the emoji/label maps with:
- `dotEmoji(live: boolean)` / `dotLabel(live: boolean)` — 2-way.
- `ringClass(remoteStatus: RemoteStatus | null)` / `ringLabel(...)` — 3-way, `null` → no ring rendered.
Keep `formatSince` as-is.

### `frontend/src/api/entities.ts`, `frontend/src/api/admin.ts`
Update `ServiceLiveStatus` / `SubscriptionLiveStatus` / `LiveConnectionRow` types: `status: LiveConnectionStatus` → `live: boolean; remote_status: RemoteStatus | null`.

### `frontend/src/components/StatusDot.vue`
- Props become `{ live: boolean; remoteStatus?: RemoteStatus | null }` (default `null`/omitted → no ring, so service rows need no changes at call sites beyond passing `live`).
- Template: outer wrapper renders the ring (border/box-shadow colored per `remoteStatus`, or no ring element when `null`); inner dot renders green/gray per `live`, keeping the existing emoji approach.
- Compose a combined `aria-label`/`title` describing both channels (e.g. "Live · remote not providing port yet") so accessibility isn't lost by splitting the single label.

### Page consumers
- `frontend/src/pages/DashboardPage.vue`: `DashboardRow` (~line 12-20) gets `live: boolean; remoteStatus: RemoteStatus | null` instead of `status`; update the mapping from services/subscriptions (~line 46/57) and the template call (~line 111) to `:live="row.live" :remote-status="row.remoteStatus"`.
- `frontend/src/pages/EntityDetailPage.vue`: service list call (~line 511) passes `:live` only (no ring); the `subscriptionStatusMap` feeding `ServiceConnector.vue` (~line 639) and that component's own `<StatusDot>` usage (check `ServiceConnector.vue` template) both need the two-field update.
- `frontend/src/pages/AdminLiveConnectionsPage.vue` (~line 89): same substitution, ring only for client-role/subscription rows.

## Tests

### Backend unit tests (new, `live_connections.rs`)
Cover all branches of the two new pure helper functions directly (no DB):
- `enabled=false` → `live=false, remote_status=gray`, regardless of bridge/owner state.
- `enabled=true, has_active_bridge=true` → `live=true` (remote_status independent, check both owner online/offline sub-cases).
- `enabled=true, has_active_bridge=false, owner_online=false` → `live=false, remote_status=gray`.
- `enabled=true, has_active_bridge=false, owner_online=true, owner_port_live=false` → `live=false, remote_status=orange` (**this is the reported bug's exact scenario**).
- `enabled=true, has_active_bridge=false, owner_online=true, owner_port_live=true` → `live=false, remote_status=green` (port ready but this subscriber hasn't bridged yet — still a valid state).
- Service-row aggregation: 0 subscribers live → `live=false`; ≥1 subscriber live → `live=true`; `remote_status` always `None`.

### Rust e2e (`crates/t2t/tests/tunnel_e2e.rs`)
Reuse `wait_for_port_config` (~line 296) and `wait_for_active_tunnel_entry` (~line 321). Extend/add scenarios asserting the live-connections API response fields directly:
- Extend `two_ssh_connections_tunnel_through_rendezvous` (~line 352): after the bridge is confirmed live, assert `subscriptions[].live == true` and (if owner online) `remote_status == "green"`.
- New scenario covering **Bug 1 exactly**: authenticate the owner's SSH session without issuing `tcpip_forward` for the target port (small new helper needed — an SSH connect/auth that skips the forward request, adapted from existing `spawn_ssh_*` helpers), then assert the subscriber's row shows `live=false, remote_status="orange"`.
- Extend `same_connection_online_target_not_blocked_by_offline_target_timeout` (~line 837) or add alongside it: fully offline owner → `live=false, remote_status="gray"`.
- Full teardown/disconnect case: reuse the existing drop pattern (~line 737-765), assert `live=false, remote_status="gray"` post-teardown.

### Frontend specs
- `frontend/src/components/StatusDot.spec.ts`: replace the 3 emoji-only cases with the `(live, remoteStatus)` matrix — `(true, null)`, `(false, null)`, `(false, "orange")` (the bug case), `(false, "gray")`, `(true, "green")` — assert dot color/emoji from `live` and ring presence/color from `remoteStatus` independently, plus the combined aria-label.
- `frontend/src/pages/DashboardPage.spec.ts`: update fixtures from `status: 'orange'` etc. to the new two-field shape; add a case asserting a row with `remoteStatus: 'orange'` renders a ring-bearing `StatusDot` while `live=false`.

## Migrations
None — this is purely a query-composition + API-shape + frontend-rendering change; `entity_status`/`entity_statuses` and `server_slots`/`active_tunnels` are existing data sources.

## Verification
- `cargo test -p t2t --test tunnel_e2e` for the e2e scenarios, plus `cargo test -p tunnel2tunnel-web` for the new unit tests.
- `cd frontend && npm run test` (or the project's vitest command) for `StatusDot.spec.ts` / `DashboardPage.spec.ts`.
- Manual check: run the app per CLAUDE.md's "Running locally" section, connect a client with `-R` but withhold the forward briefly (or use the new e2e helper's approach manually via `ssh` flags) to visually confirm gray dot + orange ring, then confirm it flips to green dot once actually bridged.

## Todos

- [x] Backend: live_connections.rs two-channel status
- [x] Backend: unit tests for status helper functions
- [x] Backend: e2e scenarios in tunnel_e2e.rs
- [x] Frontend: liveStatus.ts + api types
- [x] Frontend: StatusDot.vue dual-channel rendering
- [x] Frontend: update page consumers
- [x] Frontend: update specs
