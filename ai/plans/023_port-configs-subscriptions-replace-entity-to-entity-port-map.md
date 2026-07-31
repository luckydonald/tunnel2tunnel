# Port-configs + subscriptions: replace entity-to-entity port mapping with a direct M2M

## Context

Today, connecting a client entity to a server entity is two disconnected steps: an `entity_access` rule grants whole-entity access, and separately the client hand-types a `local_port`/`proxy_port` pair on their own `EntityPort` row, hoping the numbers line up with something the server actually exposes. A partial fix (`entity_port_discovery_rules`, `reachable-servers`) exists but bolts a discovery layer on top of that same two-sided-config model rather than replacing it, and nothing enforces port-level access at the SSH layer at all — `channel_open_direct_tcpip`/`tcpip_forward` only match raw port numbers, gated by whole-entity `entity_access`.

During planning, the user redirected away from "promote discovery within the existing model" toward a more fundamental fix: **there should be no separate listening-port config on the subscriber side at all.** A port is declared once, by whichever entity offers it (a "service"), and any other entity that wants it just registers "route this to me, on this local port" via a join row — no mirrored config, no coincidental number-matching. Critically, `entity.type` ('server'/'client') stops being a capability gate: **the same entity can simultaneously offer ports (server-like) and subscribe to others' ports (client-like)** — e.g. a laptop that both shares a local dev server with a friend and subscribes to a friend's VNC port. `type` survives only as a display tag.

## New data model

### `port_configs` (replaces `entity_ports`, migration renames the table)

The single declaration of "entity X offers this port as a service." Owned by whichever entity declares it — no longer tied to the entity being "a server."

```sql
ALTER TABLE entity_ports RENAME TO port_configs;
ALTER TABLE port_configs DROP COLUMN server_entity_id;  -- the "client mirrors a server port" concept is gone
UPDATE port_configs SET name = 'Unnamed Service' WHERE name IS NULL;
ALTER TABLE port_configs ALTER COLUMN name SET NOT NULL;  -- can be a real NOT NULL now: every port_config
                                                            -- is a declared service by construction, no more
                                                            -- nullable client-mirrored rows to accommodate
```
Remaining columns unchanged: `id, entity_id (owner), enabled, local_port, proxy_port, name, description, host, sort_order, timestamps`.

### `port_subscriptions` (new, replaces `entity_port_discovery_rules` entirely)

The M2M: "entity B wants port_config P routed to itself, on this local port."

```sql
CREATE TABLE port_subscriptions (
    id                    UUID PRIMARY KEY DEFAULT uuidv7(),
    port_config_id        UUID NOT NULL REFERENCES port_configs(id) ON DELETE CASCADE,
    subscriber_entity_id  UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    subscriber_local_port INTEGER NOT NULL,
    enabled               BOOL NOT NULL DEFAULT true,
    created_at            TIMESTAMPTZ NOT NULL,
    updated_at            TIMESTAMPTZ NOT NULL,
    UNIQUE (port_config_id, subscriber_entity_id)
);
CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON port_subscriptions
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();
```
`subscriber_local_port` is editable any time — this is the conflict-override escape hatch the user still wants, just expressed as "edit your subscription row" instead of a separate manual-mapping form. There is no more "auto/enabled/disabled" three-state: a row's mere existence + `enabled` is the whole state (no row = not subscribed at all; `enabled=false` = paused without losing the chosen local port).

### `entity_access` gains optional port scoping

```sql
ALTER TABLE entity_access ADD COLUMN port_config_id UUID REFERENCES port_configs(id) ON DELETE CASCADE;
-- NULL = grant covers the whole owner entity (today's behavior, unchanged).
-- set   = grant is scoped to exactly that one port_config.
```
This is deliberately layered under `port_subscriptions`, not a replacement for it:
- `entity_access` (extended) answers **"is entity/friend B even allowed to reach this, and how much of it"** — whole entity, or just one specific port — reusing the existing `subject_type` machinery (`public_lite`/`all_mine`/`all_user_entities`/`entity`) so friendship-driven sharing ("share all my entities," "a few selected entities," "only specific ports") all go through the same rule table, just with `port_config_id` set or left null.
- `port_subscriptions` answers **"did B actually opt in, and what local port do they want"** — the explicit, per-entity "route this to me" registration. Creating a subscription requires the caller to currently satisfy an `entity_access` check (whole-entity or port-scoped) for the target port_config's owner; this is enforced both at subscription-creation time (web route) and again live at SSH-connect time (see below), so revoking access also revokes function even if a stale subscription row is left behind.
- **Same-account access is implicit and bypasses `entity_access` entirely**: whenever `client_user_id == owner_user_id`, both the subscription-creation check and the SSH-layer live re-check short-circuit to "allowed" without consulting `entity_access` at all — you always have access to your own services on your own other entities, with nothing to configure. `entity_access` rows only ever matter for cross-account sharing (friends/public).

Migration `014_port_configs_subscriptions.sql` performs all three changes above (rename+cleanup port_configs, drop `entity_port_discovery_rules`, create `port_subscriptions`, extend `entity_access`) as one clean-break migration — no data-preservation path, matching the pre-production state of this feature.

### `entity.type` is removed from the database — badges are computed

Rather than keeping `type` as an inert tag column, migration `015_drop_entity_type.sql` drops `entities.type` entirely (`ALTER TABLE entities DROP COLUMN type`), along with the now-meaningless `entity_type` param in `CreateEntityBody`/`UpdateEntityBody`. "Server"/"Client" become **computed booleans** in the entity response, derived live from what the entity actually owns:
- `is_server: bool` — this entity owns at least one `port_configs` row.
- `is_client: bool` — this entity owns at least one `port_subscriptions` row.

An entity can show both badges, one, or neither (a freshly-created entity with nothing configured yet shows neither). `crates/tunnel2tunnel-web/src/routes/entities.rs`'s `EntityResponse` gains these two fields (computed via a `COUNT(...) > 0` subquery or a join, alongside the existing `online`/`last_disconnected_at` derivation pattern).

Frontend: `frontend/src/pages/EntityDetailPage.vue`'s create-entity form drops the type selector entirely — an entity starts as neither role and becomes one/both organically as services/subscriptions are added. `entityTypeLabel` in `labels.ts` is repurposed as a label for the *computed* badge, not a stored field. The separate "Servers"/"Clients" nav items and pages become the same merged "Entities" list page, reached via `/servers` and `/clients` routes that just pre-apply a `role=server`/`role=client` filter (query param) over the same list component/endpoint — so the quick-access muscle memory from today's separate pages still works, it's just a filtered view now instead of a distinct stored dimension.

## Backend enforcement (SSH layer)

`crates/tunnel2tunnel-ssh/src/lib.rs`:

- **`tcpip_forward`** (owner entity registers `-R proxy_port:...`): unchanged mechanically (still inserts into `server_slots` keyed by `(entity_id, proxy_port)`), but now auto-creates a `port_configs` row when none exists for `(entity_id, port)` — `local_port = port`, `proxy_port = port`, `host = "localhost"`, `enabled = true`, `name = guess_service_name(port).unwrap_or("Unnamed Service")` (new `crates/tunnel2tunnel-core/src/port_names.rs`, a static lookup table covering classic well-known ports plus common self-hosted/dev services — VNC, Redis, Postgres, Grafana, Jellyfin, Home Assistant, Plex, Node dev servers, etc.). This means running `-R 5900:...` with no prior UI setup immediately produces a real, named, enabled service — nothing is ever left in an "unconfigured port" limbo state.
- **`channel_open_direct_tcpip`** (a subscriber's `-L` fires a connection attempt): resolves the target entity as today (UUID or `entity_access.hostname` alias), then:
  1. Look up the matching enabled `port_configs` row for `(target_entity_id, port_to_connect)` — reject immediately if none (this is a real config/authorization miss, distinct from "server offline").
  2. Look up an **enabled** `port_subscriptions` row for `(that port_config.id, client_entity.id)` — reject immediately if none. This is the actual "did this entity opt in" check.
  3. Skip straight to step 4 if `client_user_id == owner_user_id` (same account); otherwise re-verify `entity_access` (extended, port-scoped-aware) live for this exact `(owner_entity_id, port_config_id, client_entity_id, client_user_id)` — reject immediately if the access has since been revoked, even though a subscription row still exists.
  4. **Server-liveness is retried, not rejected outright**: look up `server_slots` for `(target_entity_id, proxy_port)`. If missing, poll on a short interval (e.g. every 250ms) up to a bounded timeout (e.g. 10-15s, worth making configurable) before giving up — a subscriber whose service is correctly configured and authorized but whose target server just hasn't registered yet (races on startup, a server mid-restart) gets bridged automatically the moment it appears, no reconnect needed. Only after the timeout elapses with still no `server_slots` entry does this step reject.
- Steps 1-3 stay fail-fast because they're genuine authorization/config problems that won't resolve themselves by waiting. Step 4 is the only "wait and hope" case, matching the requirement that a subscriber can stay parked on a not-yet-live service and have it start working transparently. Because `channel_open_direct_tcpip` re-fires independently on every individual connection through an already-open `-L` tunnel (not once per SSH session), even a request that times out at step 4 doesn't require reconnecting — the next inbound connection to the subscriber's local port just tries again from scratch.

`crates/tunnel2tunnel-core/src/models/`:
- Rename `entity_port.rs`'s model to match the renamed table (or keep the file name, just update the struct/queries) — drop `server_entity_id` field, add `find_enabled_by_entity_and_proxy_port(pool, entity_id, proxy_port)`.
- Delete `entity_port_discovery_rule.rs`; add `port_subscription.rs` — CRUD (`create`, `update` for `subscriber_local_port`/`enabled`, `delete`, `find_by_subscriber_and_port_config`) plus the browse query `list_subscribable_for_entity(pool, subscriber_entity_id, subscriber_user_id) -> Vec<SubscribableOwner>` (replaces `list_reachable_for_client`): unions whole-entity grants and port-scoped grants from the extended `entity_access`, grouped by owning entity, each with its `port_configs` (whichever the grant covers) and whether/how the subscriber has already subscribed.
- `crates/tunnel2tunnel-core/src/models/entity_access.rs::check_access`: extend signature to accept an optional `port_config_id`, matching `(port_config_id IS NULL OR port_config_id = $target)` in the `WHERE` alongside the existing subject-type predicate.

`crates/tunnel2tunnel-web/src/routes/`:
- `entities.rs`: rename/adjust the ports CRUD routes for `port_configs`; drop `reachable-servers`/`port-discovery` routes, replace with `GET /api/entities/{id}/subscribable-services` (the browse list) and `POST/PUT/DELETE /api/entities/{id}/subscriptions` (create/update-local-port-or-enabled/delete a `port_subscriptions` row) — creation validates the extended `entity_access` check server-side before inserting.
- `access.rs`: `create_access` gains an optional `port_config_id` in its request body, validated to belong to the `owner_entity_id` being granted on.

## Frontend restructure

- `EntityDetailPage.vue` becomes uniform for every entity (no more type-branched sections):
  1. **"My services"** — the `port_configs` this entity owns; same CRUD table as today's Ports section, minus the old `server_entity_id` picker (gone) and the type-conditional Host-field-vs-picker branch (Host is now always shown, since offering a service always means declaring where it forwards to). Both `Port` (proxy_port) and `Name` are required (mirroring the DB's new `NOT NULL` on `name`, and `proxy_port`'s existing non-null column) — required-ness is conveyed via helper text/caption, not an inline `*` marker. The add-service form leads with `Port`; `Local port` auto-mirrors whatever's typed into `Port` for as long as the user hasn't separately edited `Local port` (then it decouples and stays independently editable); `Name` auto-fills from `guess_service_name(port)` the same way — synced to `Port` until the user types into `Name` directly, and left **empty** (not a placeholder string) when no guess exists, so the "Unnamed Service" fallback only ever shows up as a genuine stored value (e.g. via the migration backfill), never as a live-typed default.
  2. **"My subscriptions"** — replaces the old `SshCommandDisplay`-embedded discovery section: browse `subscribable-services` (grouped by owning entity), each port_config showing a subscribe/unsubscribe toggle and, once subscribed, an editable "my local port" field (the conflict-override case, now just editing the subscription row directly — no separate advanced/manual form needed since this *is* the only mechanism). A primary filter defaults to **"Configured"** (only services already subscribed — nothing unsubscribed clutters the default view); switching to **"Unconfigured"** or **"All"** reveals a second filter, **Origin** (`Mine` / `Friends` / `All`, defaulting to `All`), to narrow the larger unfiltered browse set by whose services they are.
  This becomes the shared `frontend/src/components/ServiceConnector.vue` component from earlier drafts, just rebuilt on `subscribable-services`/`subscriptions` instead of `reachable-servers`/`port-discovery`.
- `SshCommandDisplay.vue`: direction is now derived **per item**, not per entity — every owned `port_configs` row contributes a `-R` flag, every own `port_subscriptions` row contributes a `-L` flag, both can appear in the same generated command for the same entity. This removes the old `entity.entity_type === 'server' ? Remote : Local` branch entirely.
- Access-rule UI (`EntityDetailPage.vue`'s Access Rules section): add a scope control when creating a rule — "Whole entity" (today's default, `port_config_id = null`) vs "Specific port" (a picker over this entity's own `port_configs`, shown only if it has any).
- "Servers"/"Clients" nav items and list pages merge into a single "Entities" list page, with the computed `is_server`/`is_client` badges rendered as small tag/filter chips (see `entity.type` section above).
- `frontend/src/api/entities.ts`: `EntityPort` → `PortConfig` type (drop `server_entity_id`), new `PortSubscription`/`SubscribableOwner` types replacing `DiscoveredPort`/`ReachableServer`; API functions renamed to match the new routes.

## GUI mockups

> ```
> ── Entities ──────────────────────────────────────────────────────────────
> Filter:  ( All )  ( Server )  ( Client )              🔍 [ search... ]
> ────────────────────────────────────────────────────────────────────────
>  Name              Roles               Online   Services   Subscriptions
>  ────────────────  ──────────────────  ───────  ─────────  ─────────────
>  home-nas          🖧 Server            🟢       3          0
>  my-laptop         💻 Client            🟢       0          2
>  build-box         🖧 Server 💻 Client   🟢       1          1
>  old-vps           🖧 Server            ⚪       2          0
>                                                          [+ New entity]
> ```
> `/servers` = this same list/component with the filter preset to "Server"; `/clients` → "Client". The "Roles" column (`is_server`/`is_client`) is computed by the backend, not stored. The same two badges (🖧 Server / 💻 Client) are reused everywhere an entity's role needs labeling — including in place of "server leg"/"client leg" wording in the live-connections tables below.

> ```
> ── Entities / build-box ───────────────────────────────────────────────
> build-box   🖧 Server  💻 Client   🟢 Online
> A dev box that shares its local Postgres and subscribes to VNC.
>                                                       [Delete entity]
>
> ── SSH command ─────────────────────────────────────────────────────────
> ssh -i ~/.ssh/t2t_build-box \
>   -R 5432:localhost:5432 \        # Postgres — offered by build-box
>   -L 5901:home-nas:5900 \         # VNC — subscribed from home-nas
>   3f9a1c2e-...@t2t.example.com -p 2222
>
> ── My services ──────────────────────────────────────── [+ Add service] ─
>  ●   Service    Proxy port  Local port  Host        Subscribers      ⋮
>  🟢  Postgres   5432        5432        localhost   1 connected     ✎ ×
>  ⚪  Redis(off) 6379        6379        localhost   0                ✎ ×
>      ▸ Postgres subscribers: my-laptop (alice), local port 5433, 12m
>
>  + Add service:
>    Port        [ 5900 ]                (required)
>    Local port  [ 5900 ]                (mirrors Port until you edit it)
>    Name        [ VNC                ]  (required; auto-suggested from Port until you edit it)
>    Host        [ localhost           ]
>    Enabled     [x]                              [ Cancel ]  [ Add ]
>
> ── My subscriptions ─────────────────────────────────────────────────────
>  Show: ( Configured )  Unconfigured  All
>
>  ▾ home-nas  🖧 Server
>     🟢  VNC     proxy 5900  → my local port [ 5901 ]   [Unsubscribe]
>
>  ▾ old-vps  🖧 Server, offline
>     🟠  Postgres proxy 5432 → my local port [ 5555 ]   [Unsubscribe]
>        server is offline — will connect automatically once it's back
>
>  ── (switching Show to "Unconfigured" or "All" reveals a second filter) ──
>  Show: Configured  ( Unconfigured )  All        Origin: Mine  Friends  ( All )
>
>  ▾ home-nas  🖧 Server
>     ⚪  Samba    proxy 445   →                          [ Subscribe ]
>
>  ▾ build-box  🖧 Server (mine)
>     ⚪  Grafana  proxy 3000  →                          [ Subscribe ]
>
> ── Connection log ────────────────────────────────────────────────────────
>  (unchanged from today — per-entity SSH login/auth attempt history: peer IP,
>  key fingerprint, success/fail reason, started/ended. Kept as-is, separate
>  from the live-connections/status-dot views above, which are about active
>  tunnels rather than login attempts.)
>
> ── Access rules — who else can subscribe to my services ────────────────
> Your own entities always have access to each other automatically.
> Rules below are only needed to share with other accounts.
>
>  Grant                 Scope              Hostname alias    ⋮
>  Anyone (public_lite)  Whole entity       nas.local         ×
>  Friend: bob           Only "Postgres"    —                 ×
>  All my own entities   Whole entity       —                 ×
>                                                    [+ Add access rule]
>
>  + Add access rule:
>    Grant to  [ One specific entity ▾ ]  ...entity picker...
>    Scope     ( Whole entity )  ( Only this service: [ Postgres ▾ ] )
>    Hostname alias (optional) [                            ]
>                                          [ Cancel ]  [ Add rule ]
> ```
> Status-dot legend: 🟢 live now · ⚪ configured/available, not live · 🟠 subscribed, but the other side isn't live yet (includes the "waiting, will retry" case from the SSH-layer retry behavior above).

> ```
> ── Dashboard ─────────────────────────────────────────────────────────────
> Welcome, alice.
>
> Your live connections
>  ●   Entity      Role        Service    Port    Since
>  🟢  home-nas    🖧 Server    VNC        5900    2h 3m
>  🟢  my-laptop   💻 Client    VNC        5901    2h 3m
>  🟠  my-laptop   💻 Client    Postgres   5555    waiting…
>                                                        [See all →]
> ```

> ```
> ── Admin / Live connections ─────────────────────────────────────────────
> Filter:  [ user ▾ ]  [ role ▾ ]  [ service ▾ ]
>
>  ●   Account   Entity       Role        Service    Port   Peer IP  Since
>  🟢  alice     home-nas     🖧 Server    VNC        5900   —        2h 3m
>  🟢  alice     my-laptop    💻 Client    VNC        5901   1.2.3.4  2h 3m
>  🟠  alice     my-laptop    💻 Client    Postgres   5555   1.2.3.4  waiting…
>  ⚪  bob       build-box    🖧 Server    Redis      6379   —        —
> ```
> Each live-connections row represents one leg of a tunnel (one entity's side of it); the "Role" column just reuses the same 🖧 Server / 💻 Client badge from the Entities list rather than separate "server leg"/"client leg" wording — since an entity can hold both badges, this column simply shows which role this particular row is acting in.

## Live connections dashboard

Unchanged in spirit from the earlier draft, just built on the new tables:
- In-memory `ActiveTunnels` map in `tunnel2tunnel-ssh`, populated/torn down alongside `channel_open_direct_tcpip`'s bridge lifecycle, exposed via new `crates/tunnel2tunnel-web/src/routes/live_connections.rs` (`GET /api/entities/{id}/live-connections`, `GET /api/admin/live-connections`) — needs `ActiveTunnels`/`ServerSlots` threaded into `AppState` via `crates/t2t/src/main.rs`.
- Status dot: green = live entry exists for that `(entity, port)`; gray = configured (a `port_configs` row, or an enabled `port_subscriptions` row) with no live entry; orange = subscriber has an enabled subscription but the owner's port has no live `server_slots` entry (server side down).
- Per-entity page view: augment the "My services" table with a status dot + list of currently-connected subscribers per row; augment "My subscriptions" with a status dot per subscribed row (scoped to just this entity's own subscriptions, not a dump of everyone else on that owner).
- Admin: separate flat table, one row per leg (own `AdminLiveConnections.vue` page, following `AdminUsers.vue`/`PurgeAccess.vue` conventions) — a merged one-row-per-tunnel table was considered and rejected because it can't cleanly represent the half-connected states (subscriber configured but owner offline, or vice versa) that the status dots exist to show.
- The bubble/arrow diagram sketch stays explicitly deferred as a follow-up — the row data this phase produces is exactly what such a diagram would consume later.

## Sequencing

1. **Schema + core models**: migration `014_port_configs_subscriptions.sql` (rename/clean up `entity_ports` → `port_configs`, drop `entity_port_discovery_rules`, create `port_subscriptions`, extend `entity_access` with `port_config_id`) and `015_drop_entity_type.sql` (drop `entities.type`); update `tunnel2tunnel-core` models (`port_config.rs`, new `port_subscription.rs`, extended `entity_access.rs`, computed `is_server`/`is_client` query support).
2. **SSH-layer enforcement**: `tcpip_forward` auto-create + naming, `channel_open_direct_tcpip`'s four-step check (port_config match → subscription → same-account-bypass/live access re-check → bounded-retry server-liveness lookup).
3. **Web routes**: new/renamed CRUD + browse/subscribe endpoints, extended access-rule creation, `EntityResponse`'s computed badge fields, `role=server|client` list filtering.
4. **Frontend restructure**: merged Entities list (+ `/servers`/`/clients` as filtered views), rebuilt `EntityDetailPage.vue` (My services / My subscriptions / access-rule scope picker per the mockups above), reworked `SshCommandDisplay.vue`, entity-creation form drops the type selector.
5. **Live connections dashboard**: backend tracking + routes, then the three frontend surfaces (per-entity augmentation ×2, Dashboard, admin page) per the mockups above.

Steps 1-3 are a connected unit (schema and enforcement should land together so nothing references half-migrated tables). Step 4 depends on step 3's route shapes. Step 5 depends on steps 1-3 existing (status dots are meaningless without real enforcement) but is otherwise independent of step 4's UI details.

## Verification

- Backend: exercise the four-step SSH check manually with local SSH clients — (a) a service owner doing `-R` on an unconfigured port confirms auto-create+naming; (b) a subscriber doing `-L` with no subscription row confirms immediate rejection; (c) with a subscription but after the granting `entity_access` row is deleted (cross-account case) confirms live re-check rejection; (d) a same-account subscription works with zero `entity_access` rows configured; (e) starting the subscriber before the server confirms the bounded retry bridges the tunnel once the server registers, without restarting the SSH session; (f) the normal happy path. `cargo test` for the new model methods.
- Frontend: `npm run build` for type-checking; manually walk through an entity that both owns a port_config and holds a subscription to another entity's port_config, confirming the generated SSH command contains both a `-R` and a `-L` flag, that both badges render correctly, that `/servers`/`/clients` filter as expected, and that the access-rule port-scope picker and My-services/My-subscriptions sections match the mockups above end-to-end in a browser.
