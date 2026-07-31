# Service-oriented port discovery, enforcement, and live connection visibility

## Context

Today, connecting a client entity to a server entity is two disconnected steps: an `entity_access` rule grants whole-entity access, and separately the client hand-types a `local_port`/`proxy_port` pair on their own `EntityPort` row, hoping the numbers line up with something the server actually exposes. A partial fix for this already exists in the codebase (`entity_port_discovery_rules` table, `GET /api/entities/{id}/reachable-servers`, the "discovery-section" in `SshCommandDisplay.vue`) but it's a secondary, easy-to-miss UI tucked inside the SSH-command box, and it isn't enforced anywhere — the actual SSH routing layer (`channel_open_direct_tcpip`/`tcpip_forward` in `tunnel2tunnel-ssh`) never consults `entity_ports` at all; it just matches raw port numbers between whatever a server registered and whatever a client asks for, gated only by whole-entity `entity_access`.

The goal of this change is to flip the primary flow: a server names its ports as concrete services, and a client's primary action is "pick server X, service Y" rather than "configure a port mapping and hope it matches." This also closes a real authorization gap (port-level access isn't enforced today) and adds visibility into which tunnels are actually live right now, which doesn't exist at all currently (only whole-connection login history via `connection_logs`).

## Phase A — Promote port-discovery to the primary UX, service naming

**A1. Server ports require a name ("service"), enforced by the API.**
- `crates/tunnel2tunnel-web/src/routes/entities.rs::create_port`/`update_port`: when the target entity's `entity_type == "server"`, reject (`WebError::BadRequest`) if `name` is `None`/empty. Client-side ports stay exempt (they inherit the server port's name via discovery).
- No DB `NOT NULL` — existing rows and any still-null values stay valid at the schema level; this is an API-level rule.

**A2. Auto-naming from a port dictionary, for anywhere a name gets filled in automatically** (used by A1's UX, by discovery auto-mirroring, and by B2's auto-create path below).
- New `crates/tunnel2tunnel-core/src/port_names.rs`: a `fn guess_service_name(port: i32) -> Option<&'static str>` backed by a static table covering classic well-known ports and common self-hosted/dev services, e.g.:

  | Port | Name | Port | Name |
  |---|---|---|---|
  | 21 | FTP | 8000 | HTTP (dev) |
  | 22 | SSH | 8006 | Proxmox |
  | 23 | Telnet | 8080 | HTTP (alt) |
  | 25 | SMTP | 8081 | HTTP (alt 2) |
  | 53 | DNS | 8096 | Jellyfin |
  | 80 | HTTP | 8123 | Home Assistant |
  | 110 | POP3 | 8384 | Syncthing |
  | 143 | IMAP | 8443 | HTTPS (alt) |
  | 443 | HTTPS | 8880 | phpMyAdmin |
  | 445 | SMB | 8888 | Jupyter |
  | 465 | SMTPS | 9000 | Portainer / MinIO |
  | 587 | SMTP (submission) | 9090 | Prometheus |
  | 993 | IMAPS | 9091 | Transmission |
  | 995 | POP3S | 9100 | Node exporter |
  | 1433 | MSSQL | 9200 | Elasticsearch |
  | 1521 | Oracle DB | 11211 | Memcached |
  | 2049 | NFS | 25565 | Minecraft |
  | 3000 | Node.js (dev) | 27015 | Steam / Source |
  | 3001 | Grafana | 27017 | MongoDB |
  | 3306 | MySQL / MariaDB | 32400 | Plex |
  | 3389 | RDP | 5000 | Flask / dev |
  | 5432 | PostgreSQL | 5900 | VNC |
  | 6379 | Redis | 6443 | Kubernetes API |
  | 7777 | Game server |  |  |

  Fall back to `None` (caller then uses `"Unnamed Service"`) for anything unlisted. This table can grow over time; keep it a plain `match`/static array, not a DB table, since it's a naming *heuristic*, not user data.
- `migrations/014_entity_port_default_names.sql`: `UPDATE entity_ports SET name = 'Unnamed Service' WHERE name IS NULL;` — a plain, non-lookup backfill (migrations can't call Rust logic), matching the requirement to just label pre-existing null names generically. New rows created after this point always get a real guessed/explicit name (A1 + A2 + B2).

**A3. Frontend: reorder `EntityDetailPage.vue` so discovery is primary, manual entry is secondary.**
- Extract the interactive "discovery-section" markup out of `SshCommandDisplay.vue` (~lines 152-210) into a new `frontend/src/components/ServiceConnector.vue`. Props: `servers: ReachableServer[]`; emits `discovery-state-change` same as today. `SshCommandDisplay.vue` keeps its `discoveryFlags`/`enabledDiscoveryPortIds` computed logic (still needed to render `-L` flags in the generated command), just drops the now-extracted interactive picker markup.
- `EntityDetailPage.vue`: for client entities, render `<ServiceConnector>` near the top of the page (above the SSH command block), wired to the existing `handleDiscoveryStateChange` handler and `reachableServers` state that already exist there.
- The existing manual "Add port" inline form (typed `local_port`/`proxy_port`, ~lines 448-472) gets wrapped in a collapsed `<details>` labeled "Advanced: manual port mapping" for client entities — kept for override/conflict cases, no longer the first thing shown. For server entities the Ports table stays primary as-is (a server still hand-authors its services), but its `name` field/column becomes visually first/prominent (renamed header "Service") and required client-side too (instant validation mirroring A1, avoiding a round-trip 400).
- `frontend/src/labels.ts` or a small new helper: `portDisplayName(port: EntityPort) => port.name ?? '—'` for consistent rendering everywhere a port is listed (replace ad hoc `port.name || ...` spots in `EntityDetailPage.vue` and `SshCommandDisplay.vue`).

## Phase B — Port-level SSH enforcement + auto-create on unconfigured forwards

**B1.** `crates/tunnel2tunnel-core/src/models/entity_port.rs`: add `EntityPort::find_enabled_by_entity_and_proxy_port(pool, entity_id, proxy_port) -> Result<Option<EntityPort>, CoreError>`.

**B2. Enforce in `channel_open_direct_tcpip`** (`crates/tunnel2tunnel-ssh/src/lib.rs`, ~line 972-1097): after the existing `EntityAccess::check_access` gate and before the `server_slots` lookup, call B1's lookup; if `None`, reject with a distinct log line ("port not configured/enabled on target entity") so it's distinguishable from "server offline" in logs. Additionally check the client's own `EntityPortDiscoveryRule` for this `(client_entity_id, server_port_id)` pair and reject if its `state == "disabled"` — this makes discovery-disabling a real per-client deny rule, not just a UI hint, per your confirmation. Since each `-L` use re-triggers `channel_open_direct_tcpip` independently (see the reconnect note above), this check runs fresh on every connection attempt — no caching/staleness concern.

**B3. Auto-create an `EntityPort` row when `tcpip_forward` registers a port with no match**, instead of warning or rejecting (`crates/tunnel2tunnel-ssh/src/lib.rs::tcpip_forward`, ~line 909-939):
- Before/alongside inserting into `server_slots`, look up `EntityPort::find_enabled_by_entity_and_proxy_port` (or any port row regardless of `enabled`, to also catch a disabled one being re-registered) for `(entity_id, port)`.
- If none exists, create one: `entity_id`, `local_port = port`, `proxy_port = port`, `host = "localhost"`, `enabled = true`, `name = guess_service_name(port).unwrap_or("Unnamed Service")`, `sort_order` = next available. This means a server operator who just runs `-R 5900:...` sees a real, named, enabled port show up in their Ports table immediately — no separate "unconfigured port" state to reconcile, and it's then immediately discoverable/reachable by authorized clients (satisfying "connect once the server comes online" with no extra step).
- Add `EntityPort::create` call reuse here (already exists in the model); no new method needed beyond wiring it into `tcpip_forward`.

## Phase C — Live tunnel tracking (backend)

**C1.** Add a second in-memory map alongside `ServerSlots` in `crates/tunnel2tunnel-ssh/src/lib.rs`:
```rust
pub type ActiveTunnels = Arc<Mutex<HashMap<Uuid, ActiveTunnelInfo>>>; // keyed by a fresh bridge id
pub struct ActiveTunnelInfo {
    pub client_entity_id: Uuid,
    pub client_user_id: Uuid,
    pub target_entity_id: Uuid,
    pub proxy_port: u32,
    pub peer_ip: String,
    pub since: OffsetDateTime,
}
```
Insert in `channel_open_direct_tcpip` right after `self.bridges.insert(...)` (~line 1079); track the bridge id alongside the existing per-`ChannelId` bridge entry so it can be removed on `channel_close`/`channel_eof`/`Drop` (mirror the existing `server_slots.retain(...)` cleanup pattern in `Drop for T2tHandler`, ~line 1196-1203). Pass `active_tunnels: ActiveTunnels` into `T2tHandler` construction the same way `server_slots` already is.

**C2. Status semantics** (per your answers, computed differently per surface — see Phase D):
- **Green**: a live entry exists in `active_tunnels`/`server_slots` for that specific `(entity, port)`.
- **Gray**: the port/service is configured (an `EntityPort` row, or — for a client — a discovery-enabled service) but has no live entry.
- **Orange** (client-facing only): the client has a service pinned/enabled (`entity_port_discovery_rules.state = 'enabled'`, or an "auto" reachable port) but `server_slots` has no entry for that server+port — i.e. the server side isn't up, so the client's next connection attempt would fail even though it's configured to work.

**C3. New API surface**, `crates/tunnel2tunnel-web/src/routes/live_connections.rs` (new file, registered in the router same as other route groups):
- `GET /api/entities/{id}/live-connections` — for a **server** entity: returns, per owned `EntityPort`, its live status plus the list of currently-bridged clients hitting it (entity, account/username, ssh-user = client entity id, connected-since) — i.e. client legs whose `target_entity_id` is this server. For a **client** entity: returns, per one of *its own* subscribed/discovery-enabled services only (not all traffic on the target server), its own live status.
- `GET /api/admin/live-connections` — cross-entity, one row per leg (both server-registered-port rows and client-bridge rows), for the admin dashboard. Follows the existing admin-gating pattern used in `tarpit.rs`.
- Requires threading `ActiveTunnels`/`ServerSlots` handles from `tunnel2tunnel-ssh` into the web crate's `AppState` (small plumbing change in `crates/t2t/src/main.rs`, where both crates are already wired together) since today those maps are private to `tunnel2tunnel-ssh`.
- Row shape includes: status, account (user), entity (id/name/type, for linking), service name (from the matched `EntityPort.name`, always populated now per Phase A/B), port in use, and connected-since where applicable. Omit a "host" column where not meaningfully knowable; populate it from `peer_ip` (client rows) / the server's registered forward address where present.

## Phase D — Live connections UI (per-surface, not one generic dump)

- **Admin** (`frontend/src/pages/AdminLiveConnections.vue`, new page following `AdminUsers.vue`/`PurgeAccess.vue` conventions, linked from `AppShell.vue`'s admin nav): flat table, **one row per leg**, status dot first column, as sketched — this is the cross-cutting "everything, everywhere" view, so per-leg rows (rather than merged) correctly represent partial states (server up/no client, client subscribed/server down) without blank placeholder columns.
- **Server entity detail page** (`EntityDetailPage.vue`): no separate live-connections table. Instead, augment the *existing* Ports table (~lines 371-425) with a status dot column per port, and — expandable per row, or a small inline list — the currently-connected clients hitting that port (account, ssh-user/client-entity, since). Auto-created ports from Phase B3 simply appear as normal rows here, nothing extra to build for that case.
- **Client entity detail page**: augment `ServiceConnector.vue` (Phase A3) so each of the client's own pinned/enabled/auto-discovered services shows a live status dot (green/gray/orange per C2) next to it — scoped strictly to this client's own subscriptions, not a dump of everything happening on the target server.
- New `frontend/src/api/liveConnections.ts` (or added to `entities.ts`): typed wrappers for the two new routes plus a shared `LiveConnectionRow`/status-dot type; a small shared `.dot.green/.gray/.orange` SCSS pattern reused across the three surfaces above (status color computed server-side per C2, not recomputed in the frontend).
- **Bubble/arrow diagram** (the `(Server1 -> 5900 -> ssh) -> (5900 -> t2t -> 5900) -> (ssh -> 5902 -> my-laptop)` sketch): explicitly deferred as a follow-up, not built in this pass — the row data these tables expose is exactly what such a diagram would consume later, so this phase's data model isn't wasted if that gets picked up afterward.

## Migration numbering

Next migration is `014_entity_port_default_names.sql` (Phase A2's backfill). No other schema changes are needed — Phase B/C/D are enforcement, in-memory tracking, and API/UI only.

## Sequencing

A and B can proceed independently/in parallel (A is UI + one validation rule; B is SSH-layer logic). C depends on nothing from A/B but is most meaningful once B's enforcement exists (a "green" dot then really means "this bridge passed the real access + port checks"). D depends on C's route shapes.

## Verification

- Backend: `cargo test` for new `EntityPort` methods (B1) and any auth-flow tests; manually exercise with two local SSH clients (a "server" registering `-R` on an unconfigured port to confirm auto-create/naming from Phase B3, and a "client" doing `-L` against an enabled vs. disabled-via-discovery port to confirm B2's enforcement) using the existing `cargo run -p t2t` local dev setup from CLAUDE.md.
- Frontend: `npm run build` for type-checking; manually walk through EntityDetailPage for both a server and a client entity in a browser, confirming the promoted `ServiceConnector` flow, the collapsed manual fallback, and the live status dots update when a real tunnel is opened/closed via the SSH clients above. Check the new admin live-connections page similarly.
