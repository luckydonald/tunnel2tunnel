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

Migration `014_port_configs_subscriptions.sql` performs all three changes above (rename+cleanup port_configs, drop `entity_port_discovery_rules`, create `port_subscriptions`, extend `entity_access`) as one clean-break migration — no data-preservation path, matching the pre-production state of this feature.

### `entity.type` stops gating capability

- Migration keeps the `CHECK(type IN ('server','client'))` column as-is (still a two-valued tag), but every place in the backend/frontend that currently *branches behavior* on it gets removed:
  - `list_reachable_servers`'s `if client.entity_type != "client" { BadRequest }` guard is deleted — any entity can browse subscribable port_configs.
  - Port-config creation is no longer implicitly "a server thing" — any entity can declare one.
  - `tcpip_forward`/`channel_open_direct_tcpip` in `tunnel2tunnel-ssh` already don't check `entity_type` today — no change needed there.
- Frontend: the separate "Servers" and "Clients" list pages/nav items merge into a single "Entities" list, with `type` shown as a small tag/filter chip (per your answer). `entityTypeLabel` in `labels.ts` is kept as a plain label for that tag.

## Backend enforcement (SSH layer)

`crates/tunnel2tunnel-ssh/src/lib.rs`:

- **`tcpip_forward`** (owner entity registers `-R proxy_port:...`): unchanged mechanically (still inserts into `server_slots` keyed by `(entity_id, proxy_port)`), but now auto-creates a `port_configs` row when none exists for `(entity_id, port)` — `local_port = port`, `proxy_port = port`, `host = "localhost"`, `enabled = true`, `name = guess_service_name(port).unwrap_or("Unnamed Service")` (new `crates/tunnel2tunnel-core/src/port_names.rs`, a static lookup table covering classic well-known ports plus common self-hosted/dev services — VNC, Redis, Postgres, Grafana, Jellyfin, Home Assistant, Plex, Node dev servers, etc.). This means running `-R 5900:...` with no prior UI setup immediately produces a real, named, enabled service — nothing is ever left in an "unconfigured port" limbo state.
- **`channel_open_direct_tcpip`** (a subscriber's `-L` fires a connection attempt): resolves the target entity as today (UUID or `entity_access.hostname` alias), then:
  1. Look up the matching enabled `port_configs` row for `(target_entity_id, port_to_connect)` — reject if none (distinct log line from "server offline").
  2. Look up an **enabled** `port_subscriptions` row for `(that port_config.id, client_entity.id)` — reject if none. This is the actual "did this entity opt in" check.
  3. Re-verify `entity_access` (extended, port-scoped-aware) live for this exact `(owner_entity_id, port_config_id, client_entity_id, client_user_id)` — reject if the access has since been revoked, even though a subscription row still exists. This keeps the "always live-checked, no caching" property the codebase already relies on elsewhere.
  4. Only then fall through to the existing `server_slots` lookup and channel bridging.
- Because `channel_open_direct_tcpip` re-fires independently on every individual connection through an already-open `-L` tunnel (not once per SSH session), none of this requires a subscriber to reconnect when a server comes online or a grant changes — the next attempt just re-evaluates all three checks fresh.

`crates/tunnel2tunnel-core/src/models/`:
- Rename `entity_port.rs`'s model to match the renamed table (or keep the file name, just update the struct/queries) — drop `server_entity_id` field, add `find_enabled_by_entity_and_proxy_port(pool, entity_id, proxy_port)`.
- Delete `entity_port_discovery_rule.rs`; add `port_subscription.rs` — CRUD (`create`, `update` for `subscriber_local_port`/`enabled`, `delete`, `find_by_subscriber_and_port_config`) plus the browse query `list_subscribable_for_entity(pool, subscriber_entity_id, subscriber_user_id) -> Vec<SubscribableOwner>` (replaces `list_reachable_for_client`): unions whole-entity grants and port-scoped grants from the extended `entity_access`, grouped by owning entity, each with its `port_configs` (whichever the grant covers) and whether/how the subscriber has already subscribed.
- `crates/tunnel2tunnel-core/src/models/entity_access.rs::check_access`: extend signature to accept an optional `port_config_id`, matching `(port_config_id IS NULL OR port_config_id = $target)` in the `WHERE` alongside the existing subject-type predicate.

`crates/tunnel2tunnel-web/src/routes/`:
- `entities.rs`: rename/adjust the ports CRUD routes for `port_configs`; drop `reachable-servers`/`port-discovery` routes, replace with `GET /api/entities/{id}/subscribable-services` (the browse list) and `POST/PUT/DELETE /api/entities/{id}/subscriptions` (create/update-local-port-or-enabled/delete a `port_subscriptions` row) — creation validates the extended `entity_access` check server-side before inserting.
- `access.rs`: `create_access` gains an optional `port_config_id` in its request body, validated to belong to the `owner_entity_id` being granted on.

## Frontend restructure

- `EntityDetailPage.vue` becomes uniform for every entity (no more type-branched sections):
  1. **"My services"** — the `port_configs` this entity owns; same CRUD table as today's Ports section, minus the old `server_entity_id` picker (gone) and the type-conditional Host-field-vs-picker branch (Host is now always shown, since offering a service always means declaring where it forwards to). `name` is required in the UI (mirroring the DB's new `NOT NULL`).
  2. **"My subscriptions"** — replaces the old `SshCommandDisplay`-embedded discovery section: browse `subscribable-services` (grouped by owning entity), each port_config showing a subscribe/unsubscribe toggle and, once subscribed, an editable "my local port" field (the conflict-override case, now just editing the subscription row directly — no separate advanced/manual form needed since this *is* the only mechanism).
  This becomes the shared `frontend/src/components/ServiceConnector.vue` component from earlier drafts, just rebuilt on `subscribable-services`/`subscriptions` instead of `reachable-servers`/`port-discovery`.
- `SshCommandDisplay.vue`: direction is now derived **per item**, not per entity — every owned `port_configs` row contributes a `-R` flag, every own `port_subscriptions` row contributes a `-L` flag, both can appear in the same generated command for the same entity. This removes the old `entity.entity_type === 'server' ? Remote : Local` branch entirely.
- Access-rule UI (`EntityDetailPage.vue`'s Access Rules section): add a scope control when creating a rule — "Whole entity" (today's default, `port_config_id = null`) vs "Specific port" (a picker over this entity's own `port_configs`, shown only if it has any).
- "Servers"/"Clients" nav items and list pages merge into a single "Entities" list page, `type` rendered as a small tag/filter chip.
- `frontend/src/api/entities.ts`: `EntityPort` → `PortConfig` type (drop `server_entity_id`), new `PortSubscription`/`SubscribableOwner` types replacing `DiscoveredPort`/`ReachableServer`; API functions renamed to match the new routes.

## Live connections dashboard

Unchanged in spirit from the earlier draft, just built on the new tables:
- In-memory `ActiveTunnels` map in `tunnel2tunnel-ssh`, populated/torn down alongside `channel_open_direct_tcpip`'s bridge lifecycle, exposed via new `crates/tunnel2tunnel-web/src/routes/live_connections.rs` (`GET /api/entities/{id}/live-connections`, `GET /api/admin/live-connections`) — needs `ActiveTunnels`/`ServerSlots` threaded into `AppState` via `crates/t2t/src/main.rs`.
- Status dot: green = live entry exists for that `(entity, port)`; gray = configured (a `port_configs` row, or an enabled `port_subscriptions` row) with no live entry; orange = subscriber has an enabled subscription but the owner's port has no live `server_slots` entry (server side down).
- Per-entity page view: augment the "My services" table with a status dot + list of currently-connected subscribers per row; augment "My subscriptions" with a status dot per subscribed row (scoped to just this entity's own subscriptions, not a dump of everyone else on that owner).
- Admin: separate flat table, one row per leg (own `AdminLiveConnections.vue` page, following `AdminUsers.vue`/`PurgeAccess.vue` conventions) — a merged one-row-per-tunnel table was considered and rejected because it can't cleanly represent the half-connected states (subscriber configured but owner offline, or vice versa) that the status dots exist to show.
- The bubble/arrow diagram sketch stays explicitly deferred as a follow-up — the row data this phase produces is exactly what such a diagram would consume later.

## Sequencing

1. **Schema + core models**: migration `014_port_configs_subscriptions.sql`; update `tunnel2tunnel-core` models (`port_config.rs`, new `port_subscription.rs`, extended `entity_access.rs`).
2. **SSH-layer enforcement**: `tcpip_forward` auto-create + naming, `channel_open_direct_tcpip`'s three-step check (port_config match → subscription → live access re-check).
3. **Web routes**: new/renamed CRUD + browse/subscribe endpoints, extended access-rule creation.
4. **Frontend restructure**: merged Entities list, rebuilt `EntityDetailPage.vue` (My services / My subscriptions), reworked `SshCommandDisplay.vue`, access-rule scope picker.
5. **Live connections dashboard**: backend tracking + routes, then the three frontend surfaces (per-entity augmentation ×2, admin page).

Steps 1-3 are a connected unit (schema and enforcement should land together so nothing references half-migrated tables). Step 4 depends on step 3's route shapes. Step 5 depends on steps 1-3 existing (status dots are meaningless without real enforcement) but is otherwise independent of step 4's UI details.

## Verification

- Backend: exercise the three-step SSH check manually with local SSH clients — (a) a service owner doing `-R` on an unconfigured port confirms auto-create+naming; (b) a subscriber doing `-L` with no subscription row confirms rejection; (c) with a subscription but after the granting `entity_access` row is deleted confirms live re-check rejection; (d) the normal happy path. `cargo test` for the new model methods.
- Frontend: `npm run build` for type-checking; manually walk through an entity that both owns a port_config and holds a subscription to another entity's port_config, confirming the generated SSH command contains both a `-R` and a `-L` flag, and that the merged Entities list / access-rule port-scope picker work end-to-end in a browser.
