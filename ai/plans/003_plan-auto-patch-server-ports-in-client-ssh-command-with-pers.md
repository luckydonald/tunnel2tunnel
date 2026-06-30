# Plan: Auto-patch server ports in client SSH command (with persistence)

## Context

When viewing a **client** entity's detail page, the SSH command shows `-L <local_port>:<server>:<proxy_port>` with a literal `<server>` placeholder. The goal is to auto-discover which server entities this client is authorized to reach, list their enabled ports, and reflect them in the SSH command.

Each discovered server port has three explicit states, persisted in the DB:

| State | Meaning | DB record |
|-------|---------|-----------|
| **auto (null)** | No setting — included in command based on live discovery | No row |
| **enabled** | Explicitly accepted; appears as a real `EntityPort` in the ports section below | `entity_port_discovery_rules` row (state='enabled') + linked `EntityPort` on the client |
| **disabled** | Explicitly excluded — ignored even if still discovered | `entity_port_discovery_rules` row (state='disabled') |

The `<kbd>x</kbd>` button resets any explicit state back to **auto** (deletes the rule row; if state was 'enabled', also deletes the linked client EntityPort).

---

## DB — Migration `005_port_discovery.sql`

```sql
CREATE TABLE entity_port_discovery_rules (
    id                UUID    PRIMARY KEY DEFAULT uuidv7(),
    client_entity_id  UUID    NOT NULL REFERENCES entities(id)     ON DELETE CASCADE,
    server_port_id    UUID    NOT NULL REFERENCES entity_ports(id) ON DELETE CASCADE,
    -- 'enabled' = explicitly configured, 'disabled' = explicitly excluded
    state             TEXT    NOT NULL CHECK (state IN ('enabled', 'disabled')),
    -- set when state = 'enabled': the auto-created EntityPort on the client
    client_port_id    UUID    NULL     REFERENCES entity_ports(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL,
    UNIQUE (client_entity_id, server_port_id)
);
SELECT set_timestamps('entity_port_discovery_rules');
```

---

## Backend

### Model (`crates/tunnel2tunnel-core/src/models/`)

New file **`entity_port_discovery_rule.rs`**:
- `EntityPortDiscoveryRule` struct (matches table)
- `EntityPortDiscoveryRule::upsert(pool, client_entity_id, server_port_id, state, client_port_id)`
- `EntityPortDiscoveryRule::delete(pool, client_entity_id, server_port_id)`

New method on `Entity` (or a standalone function in a new file) — **`Entity::list_reachable_for_client`**:
```sql
SELECT DISTINCT ON (e.id)
    e.*,
    ep.*,
    epdr.state,
    epdr.client_port_id,
    ea.hostname
FROM entities e
JOIN entity_access ea ON ea.owner_entity_id = e.id
LEFT JOIN entity_ports ep ON ep.entity_id = e.id AND ep.enabled = true
LEFT JOIN entity_port_discovery_rules epdr
       ON epdr.server_port_id = ep.id
      AND epdr.client_entity_id = $client_entity_id
WHERE e.entity_type = 'server'
  AND e.deleted_at IS NULL
  AND (
    ea.subject_type = 'public_lite'
    OR (ea.subject_type = 'all_mine'          AND e.user_id = $client_user_id)
    OR (ea.subject_type = 'all_user_entities' AND ea.subject_user_id = $client_user_id)
    OR (ea.subject_type = 'entity'            AND ea.subject_entity_id = $client_entity_id)
  )
ORDER BY e.id, ep.sort_order
```

Returns `Vec<ReachableServer>`:
```rust
pub struct DiscoveredPort {
    pub port: EntityPort,
    pub state: Option<String>,      // None = auto, Some("enabled"/"disabled")
    pub client_port_id: Option<Uuid>,
}

pub struct ReachableServer {
    pub entity: Entity,
    pub hostname: Option<String>,   // from matching entity_access.hostname
    pub ports: Vec<DiscoveredPort>,
}
```

### API routes (`crates/tunnel2tunnel-web/src/routes/entities.rs`)

**`GET /api/entities/{id}/reachable-servers`**
- 400 if entity is not a client
- Calls `Entity::list_reachable_for_client(pool, entity_id, session_user_id)`
- Returns `Vec<ReachableServer>` as JSON

**`PUT /api/entities/{client_id}/port-discovery/{server_port_id}`**
Body: `{ "state": "auto" | "enabled" | "disabled", "local_port"?: u16 }`

Logic:
- `"auto"`: delete discovery rule row; if the deleted row had state='enabled' and a `client_port_id`, also delete that EntityPort
- `"enabled"`: create EntityPort on client (`local_port` from body, `proxy_port` from server port) → upsert discovery rule with state='enabled', client_port_id set
- `"disabled"`: upsert discovery rule with state='disabled', client_port_id=NULL; if a previous rule had state='enabled' with a linked port, delete that EntityPort first

Register both routes in **`crates/tunnel2tunnel-web/src/lib.rs`**.

---

## Frontend API (`frontend/src/api/entities.ts`)

```typescript
export type DiscoveryState = 'auto' | 'enabled' | 'disabled'

export interface DiscoveredPort {
  port: EntityPort
  state: DiscoveryState
  client_port_id: string | null
}

export interface ReachableServer {
  entity: Entity
  hostname: string | null
  ports: DiscoveredPort[]
}

export async function getReachableServers(entityId: string): Promise<ReachableServer[]>

export async function setPortDiscoveryState(
  clientEntityId: string,
  serverPortId: string,
  state: DiscoveryState,
  localPort?: number,
): Promise<void>
```

---

## Frontend page (`frontend/src/pages/EntityDetailPage.vue`)

After loading entity, if `entity.entity_type === 'client'`:
- Call `getReachableServers(entityId)` → store as `reachableServers: Ref<ReachableServer[]>`
- Pass to `SshCommandDisplay` as `:reachable-servers="reachableServers"`
- On `@discovery-state-change` event from the component: call `setPortDiscoveryState(...)` then reload the entity (to reflect new EntityPort in the ports section) and reload reachable servers

---

## Frontend component (`frontend/src/components/SshCommandDisplay.vue`)

**New prop:** `reachableServers?: ReachableServer[]`

**Emits:** `discovery-state-change(serverPortId, state, localPort?)`

**New UI section** (client entities only, above the generated command):

```
Authorized server ports
┌──────────────────────────────────────────────────────────┐
│ ☑ server-name (or UUID)                                  │
│   ☑  3306 → proxy 5000  MySQL        [lock icon]         │  ← state: auto
│   ☑  5432 → proxy 5001  PostgreSQL   [lock icon] [x]     │  ← state: enabled
│   ☐  8080 → proxy 5002               [x]                 │  ← state: disabled
└──────────────────────────────────────────────────────────┘
```

**Visual states per port row:**
- **auto**: checkbox checked (auto-styled, e.g. slightly muted), no `<kbd>x</kbd>`. A "lock/pin" button to move to enabled.
- **enabled**: checkbox checked, solid style (port appears in ports section below), `<kbd>x</kbd>` button.
- **disabled**: checkbox unchecked, `<kbd>x</kbd>` button.

**Checkbox interactions:**
- auto → uncheck → emits `discovery-state-change(id, 'disabled')`
- disabled → check → emits `discovery-state-change(id, 'auto')` (back to auto, not enabled)
- Lock/pin button (auto → enabled) → shows a small inline `local_port` input defaulting to `server_port.local_port`, then emits `discovery-state-change(id, 'enabled', localPort)`
- `<kbd>x</kbd>` → emits `discovery-state-change(id, 'auto')` (resets to null)

**SSH command generation** — for client entities, append to command after own ports:
- Include `-L {local_port}:{hostname ?? server.entity.id}:{port.proxy_port}` for each port where state is `'auto'` or `'enabled'`

---

## Verification

1. Create server entity with 2 enabled ports + `all_mine` access rule
2. Create client entity (same user) → open client detail page
3. Both server ports appear in "Authorized server ports" section, checkboxes auto-checked, included in SSH command
4. Uncheck one → it goes disabled, disappears from command; `<kbd>x</kbd>` appears
5. Click `<kbd>x</kbd>` → back to auto (checked again, in command, no x)
6. Click lock/pin on a port → local_port input appears; confirm → port appears in the Ports section below AND in command; `<kbd>x</kbd>` visible
7. Click `<kbd>x</kbd>` on enabled port → EntityPort removed from ports section, back to auto
8. Server with `entity`-type access rule for a different client → does NOT appear for our client
9. `public_lite` server → appears for any client
