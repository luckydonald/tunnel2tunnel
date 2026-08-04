# Unify owner/subscriber live-status colors (reuse orange for "forwarded but idle")

## Context

Debug data (`ai/errors/10.client.txt` / `10.server.txt`) showed the subscriber side (Fedora Work) reporting `remote_status: "green"` for its VNC subscription, while the owner side (m1n) simultaneously reported `live: false, subscribers: []` for the exact same port. This reads as a contradiction but isn't a functional bug — it's two different signals:

- Subscriber ring (`remote_status`, `crates/tunnel2tunnel-web/src/routes/live_connections.rs:421-429`, `subscription_remote_status`) currently turns **green** as soon as the owner is online *and* has issued `tcpip_forward` for the port (`ServerSlots` has an entry keyed `(owner_entity_id, proxy_port)` — `crates/tunnel2tunnel-ssh/src/lib.rs:43,1049-1052`). It does **not** require any actual bridged traffic.
- Owner-side `live`/`subscribers` (`live_connections.rs:144-179`) is driven entirely by `ActiveTunnels` (`crates/tunnel2tunnel-ssh/src/lib.rs:52-64`), which only has an entry while a real TCP connection is currently bridged through the tunnel (populated on `channel_open_direct_tcpip`, removed on close).

So "green ring" meant only "owner offered the port," while "0 subscribers" meant "nobody is using it *right this second*" — both correct, but visually contradictory since one side implies "it's working" and the other implies "it's not."

User decision: rather than reword this into two different-colored states, make **both sides share the same 3-state color semantics** — reusing the existing `RemoteStatus::{Gray, Orange, Green}` enum (`live_connections.rs:67-73`), no new color needed:

- **Gray** — not reachable / not forwarding (owner offline, or hasn't registered this port, or subscription disabled).
- **Orange** — reachable/forwarding, but **no active bridge right now** (covers both "owner hasn't forwarded yet" and "forwarded but idle" — merged, since neither is actionable-different for the viewer).
- **Green** — an active bridge exists right now (real traffic path up).

This makes "green" mean the same thing on both sides: *there is live traffic flowing through this port right now*, matching what `live`/`subscribers` already means on the owner side. It also makes the existing `live` boolean and `remote_status` ring redundant-but-consistent on the subscriber side (both now driven off the same bridge-presence signal) instead of contradictory.

## Changes

### Backend — `crates/tunnel2tunnel-web/src/routes/live_connections.rs`

1. **`subscription_remote_status`** (currently ~421-429, signature `(enabled, owner_online, owner_port_live)`): add a `bridge_active: bool` parameter (pass `bridge.is_some()` from the call site at ~198-199, which already computes `bridge`). New logic:
   - `!enabled` → `Gray`
   - `bridge_active` → `Green`
   - `owner_online && owner_port_live` → `Orange`
   - else → `Gray`

2. **Services loop** (~144-179): stop hardcoding `remote_status: None`. After computing `live`/`subscribers`, look up `owner_port_live = live_slots.contains(&(entity_id, pc.proxy_port as u32))` (same map already used in the subscriptions loop — confirm it's in scope/computed before this loop, hoist if not) and set:
   - `live` (bridge exists) → `Green`
   - else `owner_port_live` (forwarding but idle) → `Orange`
   - else → `Gray`

   Update the struct/type doc comments (`ServiceLiveStatus.remote_status` in this file, and the mirrored comment in `frontend/src/api/entities.ts:89-98`) — no longer "always `None`."

3. **Admin flat-row builder** (~254-346) and **dashboard aggregate builder** (~372-406): these reportedly duplicate similar per-service/per-subscription logic rather than calling the two functions above — locate the duplicated logic and apply the same tri-state rule so admin/dashboard views stay consistent with entity-detail views.

### Frontend

- `frontend/src/liveStatus.ts` (ring label strings, ~lines 25-29): reword to reflect the merged Orange meaning and the stricter Green meaning, e.g.:
  - `gray` → `"Remote is not connected or not offering this port"`
  - `orange` → `"Remote is reachable, but nothing is bridged through this port right now"`
  - `green` → `"Actively bridging traffic through this port right now"`
- `frontend/src/api/entities.ts:89-98`: update the `ServiceLiveStatus.remote_status` doc comment (no longer always `null`).
- No component changes expected in `StatusDot.vue`/`EntityDetailPage.vue`/`ServiceConnector.vue` — they already read `remote_status` for both services and subscriptions; they'll just start receiving non-null/differently-computed values.

### Tests

- `crates/t2t/tests/tunnel_e2e.rs:900-1000, 1400-1430, 1600-1608` currently assert the old dot/ring split (e.g. "forwarding ⇒ green" for subscriptions, "service remote_status always null"). Update these to the new tri-state expectations:
  - subscription: forwarding-but-no-bridge ⇒ `Orange` (was `Green`); active bridge ⇒ `Green` (unchanged); not forwarding/owner offline ⇒ `Gray` (unchanged).
  - service: add assertions for `Gray`/`Orange`/`Green` based on forwarding + bridge presence (previously untested since always `None`).

## Out of scope (noted, not fixed here)

Both the owner-side match and the `ServerSlots` lookup key off `(entity_id, proxy_port_number)` rather than `port_config_id` (`live_connections.rs:149, 196-197`), while `ActiveTunnels`/the subscriber's `live` dot use the more precise `port_config_id` (`:190`). This is inherent to `ServerSlots` being keyed by literal SSH-forwarded port number (the SSH protocol has no concept of `port_config_id`), so it's not simply fixable without a broader model change. It only matters if one entity has two `port_config`s sharing the same `proxy_port`, which isn't the case here — leaving as-is.

## Verification

- `cargo test -p t2t --test tunnel_e2e` after updating the assertions above.
- Manually reproduce the original scenario: subscribe entity A to entity B's service, have B forward the port (`-R`) without any client actively connecting through A's `-L` port — confirm both A's subscription ring and B's service dot/ring show **Orange**, not a green/empty-subscriber mismatch. Then actually open a connection through A's local port and confirm both flip to **Green** together.
