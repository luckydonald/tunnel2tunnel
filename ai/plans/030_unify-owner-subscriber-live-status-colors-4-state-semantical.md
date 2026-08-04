# Unify owner/subscriber live-status colors (4-state, semantically-named enum)

## Context

Debug data (`ai/errors/10.client.txt` / `10.server.txt`) showed the subscriber side (Fedora Work) reporting `remote_status: "green"` for its VNC subscription, while the owner side (m1n) simultaneously reported `live: false, subscribers: []` for the exact same port. This reads as a contradiction but isn't a functional bug — it's two different signals:

- Subscriber ring (`remote_status`, `crates/tunnel2tunnel-web/src/routes/live_connections.rs:421-429`, `subscription_remote_status`) currently turns **green** as soon as the owner is online *and* has issued `tcpip_forward` for the port (`ServerSlots` has an entry keyed `(owner_entity_id, proxy_port)` — `crates/tunnel2tunnel-ssh/src/lib.rs:43,1049-1052`). It does **not** require any actual bridged traffic.
- Owner-side `live`/`subscribers` (`live_connections.rs:144-179`) is driven entirely by `ActiveTunnels` (`crates/tunnel2tunnel-ssh/src/lib.rs:52-64`), which only has an entry while a real TCP connection is currently bridged through the tunnel (populated on `channel_open_direct_tcpip`, removed on close).

So "green ring" meant only "owner offered the port," while "0 subscribers" meant "nobody is using it *right this second*" — both correct, but visually contradictory since one side implies "it's working" and the other implies "it's not."

User decision: make **both sides share the same 4-state semantics**, driven off the same underlying signals (`ServerSlots` forwarding-presence + `ActiveTunnels` bridge-presence) on both the owner and subscriber side. The enum's variants must be named for the **state**, not the color — color is a presentation-layer mapping the frontend applies to the state, not part of the enum's meaning:

- **`Offline`** (gray) — not reachable at all: owner not connected, or subscription disabled.
- **`NotForwarded`** (orange) — owner reachable, but hasn't registered (`tcpip_forward`) this port yet.
- **`Idle`** (blue, new) — port is forwarded (`ServerSlots` has an entry), but no active bridge right now — replaces today's misleading "green."
- **`Active`** (green) — an active bridge exists right now (real traffic path up) — matches what `live`/`subscribers` already means on the owner side.

This makes "the fully-green/active state" mean the same thing on both sides: *there is live traffic flowing through this port right now*. It also makes the existing `live` boolean and the ring status consistent with each other on the subscriber side (both now driven off the same bridge-presence signal) instead of contradictory, while preserving the previously-collapsed distinction between "owner hasn't forwarded yet" vs "forwarded but idle" as two separate, separately-colored states rather than merging them.

## Changes

### Backend — `crates/tunnel2tunnel-web/src/routes/live_connections.rs`

0. **`RemoteStatus` enum** (currently `Gray, Orange, Green` at ~67-73): rename variants to state names, add the new fourth state:
   ```rust
   #[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
   #[serde(rename_all = "snake_case")]
   pub enum RemoteStatus {
       Offline,
       NotForwarded,
       Idle,
       Active,
   }
   ```
   (`snake_case` so the wire values are `offline`/`not_forwarded`/`idle`/`active` — update the frontend `RemoteStatus` TS union type in `frontend/src/api/entities.ts` to match exactly.)

1. **`subscription_remote_status`** (currently ~421-429, signature `(enabled, owner_online, owner_port_live)`): add a `bridge_active: bool` parameter (pass `bridge.is_some()` from the call site at ~198-199, which already computes `bridge`). New logic:
   - `!enabled || !owner_online` → `Offline`
   - `bridge_active` → `Active`
   - `owner_port_live` → `Idle`
   - else → `NotForwarded`

2. **Services loop** (~144-179): stop hardcoding `remote_status: None`. After computing `live`/`subscribers`, look up `owner_port_live = live_slots.contains(&(entity_id, pc.proxy_port as u32))` (same map already used in the subscriptions loop — confirm it's in scope/computed before this loop, hoist if not) and set:
   - `live` (bridge exists) → `Active`
   - else `owner_port_live` (forwarding but idle) → `Idle`
   - else → `NotForwarded` (the service's own entity is always "online" from its own point of view when viewing its own dashboard, so `Offline` doesn't apply here — confirm this against how the entity-offline case is actually surfaced elsewhere on this page, e.g. the entity's own `online` field, and adjust if there's an existing convention to follow)

   Update the struct/type doc comments (`ServiceLiveStatus.remote_status` in this file, and the mirrored comment in `frontend/src/api/entities.ts:89-98`) — no longer "always `None`."

3. **Admin flat-row builder** (~254-346) and **dashboard aggregate builder** (~372-406): these reportedly duplicate similar per-service/per-subscription logic rather than calling the two functions above — locate the duplicated logic and apply the same tri-state rule so admin/dashboard views stay consistent with entity-detail views.

### Frontend

- `frontend/src/api/entities.ts`: update the `RemoteStatus` TS type to the new 4-value union (`'offline' | 'not_forwarded' | 'idle' | 'active'`), and update the `ServiceLiveStatus.remote_status` doc comment (no longer always `null`).
- `frontend/src/liveStatus.ts` (ring label/color mapping, ~lines 13, 25-29): add the new state, map each to a color + label:
  - `offline` → gray, `"Remote is not connected"` (subscriptions) / not expected to occur for services (see note above)
  - `not_forwarded` → orange, `"Remote is connected, but this port is not forwarded yet"`
  - `idle` → **blue** (new color — check `StatusDot.vue`'s CSS/color map for how ring colors are defined, e.g. a `RemoteStatus → hex/class` lookup, and add the blue entry there), `"Port is forwarded, but nothing is bridged through it right now"`
  - `active` → green, `"Actively bridging traffic through this port right now"`
- No structural component changes expected in `StatusDot.vue`/`EntityDetailPage.vue`/`ServiceConnector.vue` beyond adding the new color case — they already read `remote_status` for both services and subscriptions; they'll just start receiving a 4th possible value.

### Tests

- `crates/t2t/tests/tunnel_e2e.rs:900-1000, 1400-1430, 1600-1608` currently assert the old 3-state split (e.g. "forwarding ⇒ green" for subscriptions, "service remote_status always null"). Update these to the new 4-state expectations:
  - subscription: owner offline/disabled ⇒ `Offline`; owner online, not forwarded ⇒ `NotForwarded` (was `Gray`); forwarded, no bridge ⇒ `Idle` (was `Green`); active bridge ⇒ `Active` (was `Green`, unchanged meaning).
  - service: add assertions for `NotForwarded`/`Idle`/`Active` based on forwarding + bridge presence (previously untested since always `None`).

## Out of scope (noted, not fixed here)

Both the owner-side match and the `ServerSlots` lookup key off `(entity_id, proxy_port_number)` rather than `port_config_id` (`live_connections.rs:149, 196-197`), while `ActiveTunnels`/the subscriber's `live` dot use the more precise `port_config_id` (`:190`). This is inherent to `ServerSlots` being keyed by literal SSH-forwarded port number (the SSH protocol has no concept of `port_config_id`), so it's not simply fixable without a broader model change. It only matters if one entity has two `port_config`s sharing the same `proxy_port`, which isn't the case here — leaving as-is.

## Verification

- `cargo test -p t2t --test tunnel_e2e` after updating the assertions above.
- Manually reproduce the original scenario: subscribe entity A to entity B's service, have B forward the port (`-R`) without any client actively connecting through A's `-L` port — confirm both A's subscription ring and B's service ring show **Idle** (blue), not the old green/empty-subscriber mismatch. Then actually open a connection through A's local port and confirm both flip to **Active** (green) together. Also confirm B *not* forwarding at all shows **NotForwarded** (orange) on A's side.
