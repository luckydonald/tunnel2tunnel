## Summary of grounding

**Backend (`crates/tunnel2tunnel-web/src/routes/live_connections.rs`, 286 lines):**
- `status_for_subscription(enabled, has_active_bridge, owner_live)` (lines 276-286) already **is** a 3-way collapse of exactly two independent booleans: `has_active_bridge` (own/local channel live) and `owner_live` (counterpart's port-forward is registered in `server_slots`). The mapping is:
  - `!enabled` → `"gray"`
  - `has_active_bridge` → `"green"`
  - `owner_live` (but no bridge) → `"gray"`
  - else (`owner_live == false`) → `"orange"`
- This is only computed for **subscriber-side rows** (`SubscriptionLiveStatus`, both in `list_entity_live_connections`'s `subscriptions` array and in `list_admin_live_connections`'s client-role rows). Owner-side rows (`ServiceLiveStatus`, and server-role admin rows) only ever get `"green"`/`"gray"` from `live_slots.contains(...)` (lines 144, 219) — the code comment literally says "a service you own is never orange."
- Critically, `owner_live` here means **"owner's specific port has an active `server_slots` forward entry,"** not the richer `Entity.online` (SSH-authenticated-at-all) signal computed in `connection_log.rs::entity_status`/`entity_statuses` (lines 330-372) and exposed via `EntityResponse.online` in `entities.rs` (lines 94-131). Those are two genuinely different signals already, and today's `orange` conflates "owner not port-forwarding" with what the user perceives as "remote is not connected" — but it does NOT use the `entity.online` SSH-session signal at all. This is the crux of Bug 1: no code path currently cross-references `entity_status`/`entity_statuses` when computing port-level status.

**Frontend:**
- `frontend/src/liveStatus.ts` (43 lines): single `LiveStatus = 'green'|'gray'|'orange'` union, plus emoji/label maps and `formatSince`.
- `frontend/src/components/StatusDot.vue` (23 lines): single `status: LiveStatus` prop, renders one emoji span, no ring/dual-channel concept exists yet.
- `frontend/src/api/entities.ts` and `frontend/src/api/admin.ts` each **redeclare their own** `LiveConnectionStatus`/`LiveStatus` type aliases (`'green'|'gray'|'orange'`) — these need to change in lockstep with `liveStatus.ts`'s `LiveStatus` and the Rust `&'static str` status fields.
- Consumers: `DashboardPage.vue` (line 111, flattens per-entity services+subscriptions into rows, passes `row.status` straight through), `EntityDetailPage.vue` (line 511 for services, and a `subscriptionStatusMap` passed into `ServiceConnector.vue` for subscriptions), `AdminLiveConnectionsPage.vue` (line 89, straight passthrough of `LiveConnectionRow.status`).
- `StatusDot.spec.ts` (21 lines, 3 cases: green/gray/orange emoji + aria-label) will need to be largely rewritten around two props.

**Tests:** `crates/t2t/tests/tunnel_e2e.rs` (1143 lines) has full real-SSH-process scenario tests (`two_ssh_connections_tunnel_through_rendezvous` at line 352, `same_connection_online_target_not_blocked_by_offline_target_timeout` at line 837) plus polling helpers `wait_for_port_config` (296) and `wait_for_active_tunnel_entry` (321, polls the shared `ActiveTunnels` map). No unit tests exist at all for `status_for_subscription` — it's a pure, trivially-unit-testable function with zero coverage today.

---

## Design decision: what does "remote" mean

Given the grounding above, I recommend **not** inventing a new concept from scratch, but **generalizing the existing subscription-side two-signal model** and **not** attempting to force a symmetric "remote" concept onto service (owner) rows, for these reasons:

1. For a **subscription row** (I am the client), "remote" has one unambiguous referent: the single owner entity of that `port_config`. Two independent signals already exist and are already computed per-row: `has_active_bridge` (own channel) and `owner_live` (remote's port). The only change needed is to also let the owner's *entity-level* online status (`entity_status`) inform the ring, since that's the signal the user's mental model actually points at ("client shows online, but dot gray").
2. For a **service row** (I am the owner), "remote" is ambiguous by construction — a port can have 0, 1, or N concurrent subscribers, so there is no single counterpart entity to reflect in one ring. Forcing an aggregate ("any subscriber online") answers a different, less useful question and adds asymmetric special-casing for no clear payoff, since the existing `subscribers: Vec<SubscriberInfo>` list on that row already shows exactly which clients are connected.

**Recommendation:** apply the two-channel (dot + ring) rendering to **all rows uniformly** at the component level (so `StatusDot.vue` doesn't need row-type-specific logic), but have the *backend* only ever set the ring field truthy for subscription-side rows; service-side rows always get `remote_live: false` (no ring / ring never shown) — mirroring today's "a service you own is never orange" comment, just split into two booleans instead of one enum. This is the lowest-risk change that fixes both Bug 1 (ring is driven off `entity_status`/`entity_statuses`, the real SSH-online signal, not just `server_slots`) and Feature 2 (dot and ring become independently renderable/testable), while preserving 100% of current visible behavior for anyone not yet using the ring.

**Open question to raise with the user before implementing** (flagged, not decided unilaterally):
- Should the subscription-row ring be driven by `owner_live` (existing: owner's *port* is forwarding) as today, or by the owner entity's `entity.online` (SSH-authenticated at all, even if this specific port isn't forwarded yet)? These differ exactly in the bug's reported scenario. I'd recommend switching the ring to `entity.online` (broader "is the remote machine here at all") while keeping the **dot** driven by the current port/bridge-level liveness — that directly fixes Bug 1's complaint. But this is a behavior change (today's `orange` also fires when the remote entity is online but its port-forward hasn't started yet) and should be confirmed.
- Should service (owner) rows ever get a ring at all (e.g., "no client currently subscribed" indicator), or is ring strictly a subscriber-side concept as recommended above? I'm assuming the latter — flag if wrong.
- Should the existing string enum (`'green'|'gray'|'orange'`) be kept for backward compat on the wire (derive it server-side from the two new booleans for old clients) or fully replaced? I'd replace it cleanly since this is a small pre-1.0-feeling internal app judging by the code, but confirm no external API consumers depend on `status` as a string.

---

## Implementation plan (backend → frontend → tests)

### 1. `crates/tunnel2tunnel-core/src/models/connection_log.rs`
No changes needed — `entity_status` (line 333) and `entity_statuses` (line 350, batch/N+1-safe) already provide exactly the `(bool online, Option<OffsetDateTime> last_disconnected_at)` needed for the ring's "remote online" signal. Reuse `entity_statuses` (batch) in the live-connections route rather than calling `entity_status` per row.

### 2. `crates/tunnel2tunnel-web/src/routes/live_connections.rs` (primary change)
- Replace `status: &'static str` on `ServiceLiveStatus`, `SubscriptionLiveStatus`, and `LiveConnectionRow` with two fields, e.g.:
  ```rust
  pub live: bool,          // own/local channel currently live (was green vs gray)
  pub remote_online: bool, // counterpart entity currently SSH-online (ring); always false for owner-side rows
  ```
  Keep `enabled` as-is on `SubscriptionLiveStatus` (already present) — frontend derives "paused" purely from `enabled == false` regardless of the two new fields, so it stays orthogonal to the plan's row2/row1 config note.
- In `list_entity_live_connections`:
  - Batch-fetch entity statuses up front: collect all owner-entity-ids referenced by `subscriptions` (the `s.port_config.entity_id` values) into a `Vec<Uuid>`, call `ConnectionLog::entity_statuses(&state.db, &ids)` once, build a `HashMap<Uuid, bool>` of online flags.
  - `services` rows: `live = live_slots.contains(...)` (unchanged), `remote_online = false` always (services never get a ring per the recommendation above).
  - `subscriptions` rows: `live = has_active_bridge`; `remote_online = entity_statuses_map.get(&owner_entity_id).copied().unwrap_or(false)` (pending confirmation of open question #1 — if the decision is to keep the existing `owner_live` semantics instead, just keep using `live_slots` there and skip the `entity_statuses` batch call entirely — much smaller diff).
- In `list_admin_live_connections`: same two-field change for `LiveConnectionRow`; batch fetch entity_statuses for the distinct set of `s.port_config.entity_id` across all subscription rows once, reuse per row.
- Rewrite `status_for_subscription` into two pure helper functions with no `&'static str` return, e.g. `fn subscription_live(enabled: bool, has_active_bridge: bool) -> bool` and `fn subscription_remote_online(enabled: bool, remote_online: bool) -> bool` (both gated by `enabled` so a disabled subscription shows no ring/no green, matching current "disabled → gray" semantics) — these are what get unit-tested in step 4.
- Update the module doc comment at the top (lines 1-11) to describe the new two-channel model instead of the collapsed 3-way one.

### 3. Frontend types (`frontend/src/liveStatus.ts`, `frontend/src/api/entities.ts`, `frontend/src/api/admin.ts`)
- Drop the `LiveStatus`/`LiveConnectionStatus` string-union exports (or keep as a derived convenience type only for legend/testing, not for wire data).
- `entities.ts`: `ServiceLiveStatus.status: LiveConnectionStatus` → `live: boolean; remote_online: boolean`. Same for `SubscriptionLiveStatus`.
- `admin.ts`: same change to `LiveConnectionRow`.
- `liveStatus.ts`: replace `statusDotEmoji`/`statusDotLabel` maps (keyed by the old enum) with functions/maps keyed by `live: boolean` (dot) and `remote_online: boolean` (ring), e.g. `dotEmoji(live: boolean)`, `dotLabel(live: boolean)`, `ringClass(remoteOnline: boolean)` — keep `formatSince` untouched (unrelated).

### 4. `frontend/src/components/StatusDot.vue`
- Change props to `{ live: boolean; remoteOnline?: boolean }` (default `false`/omitted → no ring rendered, so service rows that never pass `remoteOnline` don't regress visually).
- Template: wrap the existing dot span in an outer span/element carrying the ring, e.g. a `box-shadow`/`border` ring in orange only when `remoteOnline` is true, main dot text/emoji still green/gray from `live`.
- Keep `role="img"`/`aria-label` on the outer element, compose the label (e.g. "Live now, remote offline" when both conditions co-occur) so accessibility isn't lost by splitting into two visual channels.

### 5. Page consumers
- `DashboardPage.vue`: `DashboardRow` interface (line 12-20) gets `live: boolean; remoteOnline: boolean` instead of `status: LiveConnectionStatus`; lines 46/57 pass through the new fields from `service`/`sub`; template line 111 becomes `<StatusDot :live="row.live" :remote-online="row.remoteOnline" />`.
- `EntityDetailPage.vue`: line 511 becomes `:live="serviceStatusMap.get(port.id)!.live" :remote-online="serviceStatusMap.get(port.id)!.remote_online"`; the `subscriptionStatusMap` passed to `ServiceConnector.vue` (line 639) and consumed at `ServiceConnector.vue:95` (`statusFor`) needs its rendering call site updated the same way — check `ServiceConnector.vue` template for its own `<StatusDot>` usage before editing (not yet read; confirm during implementation).
- `AdminLiveConnectionsPage.vue`: line 89 same substitution.

### 6. Backend unit tests (new, in `live_connections.rs` or a `#[cfg(test)] mod tests` block there)
Add direct unit tests for the two new pure helpers covering all branches:
- `enabled=false` → `live=false, remote_online=false` regardless of bridge/owner state (paused case).
- `enabled=true, has_active_bridge=true` → `live=true` (regardless of remote-online decision).
- `enabled=true, has_active_bridge=false, remote signal=true` → `live=false, remote_online=true` (the "gray dot, orange ring" case — client not yet bridged but remote is there).
- `enabled=true, has_active_bridge=false, remote signal=false` → `live=false, remote_online=false` (fully dark).
These require no DB/network — pure function tests, fast, and finally give `status_for_subscription`'s (now split) logic actual coverage.

### 7. Rust e2e scenarios (`crates/t2t/tests/tunnel_e2e.rs`)
Reuse `wait_for_port_config` and `wait_for_active_tunnel_entry` plus the existing `same_connection_online_target_not_blocked_by_offline_target_timeout` scenario (line 837) as a template for a multi-target setup. Add/extend assertions (not necessarily new top-level tests, could extend existing ones) to check the live-connections HTTP response fields directly:
- Extend `two_ssh_connections_tunnel_through_rendezvous`: after `wait_for_active_tunnel_entry` succeeds (line 711/722), add an assertion hitting `GET /api/entities/{client_entity.id}/live-connections` (or call the route function directly against `state`) verifying `subscriptions[].live == true`.
- New scenario (or extend the "offline target" test at line 837): assert the offline-target's subscription row has `live=false`; if the ring is wired to `entity.online`, assert `remote_online=false` there (matching that target's SSH session is not authenticated at all) versus a variant where the target *is* SSH-authenticated but simply hasn't set up the specific port-forward yet (`remote_online=true, live=false` — this is literally Bug 1's reported case) — this second sub-case is new and doesn't exist in any current test; it requires authenticating a target SSH session without issuing the specific `-R` forward for the port under test, which the existing helpers don't directly support and will need a small new helper (e.g. `spawn_ssh_authenticated_only` or reusing `spawn_ssh_r`/`spawn_ssh_l` machinery minus the forward request) — flag this as the one genuinely new piece of test infrastructure needed.
- Fully-disconnected case: reuse the existing drop/teardown pattern (lines 737-765) and assert both `live=false` and `remote_online=false` post-teardown.

### 8. Frontend spec updates
- `StatusDot.spec.ts`: replace the 3 emoji-only cases with cases over the `(live, remoteOnline)` matrix: `(true,false)`→green dot no ring, `(false,false)`→gray dot no ring, `(false,true)`→gray dot + orange ring, `(true,true)`→green dot + orange ring (edge case, should still render sensibly). Assert on the ring's CSS class/attribute presence, not just emoji text.
- `DashboardPage.spec.ts`: update whatever mock `DashboardRow`/API fixtures currently set `status: 'orange'` etc. to the new two-field shape; add a case asserting a row with `remoteOnline=true` renders the ring-bearing `StatusDot`.

### 9. Migrations / DB
None required — confirmed by reading `connection_log.rs`: `entity_status`/`entity_statuses` are pure derived queries over the existing `connection_logs` table (no new columns), and `live_slots`/`active_tunnels` are in-memory maps already populated by the SSH layer. This is purely a query-composition + API-shape + frontend-rendering change.

---

### Critical Files for Implementation
- /home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/live_connections.rs
- /home/user/git/luckydonald/tunnel2tunnel/frontend/src/components/StatusDot.vue
- /home/user/git/luckydonald/tunnel2tunnel/frontend/src/liveStatus.ts
- /home/user/git/luckydonald/tunnel2tunnel/frontend/src/api/entities.ts
- /home/user/git/luckydonald/tunnel2tunnel/crates/t2t/tests/tunnel_e2e.rs