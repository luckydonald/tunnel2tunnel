# Plan: Auto-patch server ports in client SSH command

## Context

When viewing a **client** entity's detail page, the SSH command currently shows `-L <local_port>:<server>:<proxy_port>` with a literal `<server>` placeholder that the user must manually replace. This is unhelpful — the user has to know which server UUIDs/hostnames they're connecting to and which proxy ports those servers expose.

The goal: automatically discover which server entities this client is authorized to reach, list their enabled ports with per-port accept/deny toggles (default: accepted/included), and reflect the selections in the generated SSH command using real server UUIDs.

---

## Approach

### 1. Backend — new "reachable servers" endpoint

**New route:** `GET /api/entities/{id}/reachable-servers`

Returns the server entities (with their enabled ports) that the given client entity is authorized to connect to, based on existing `entity_access` rules.

**SQL logic** (inline query in a new model method):
```sql
SELECT DISTINCT e.*, array_agg(ep.*) as ports
FROM entities e
JOIN entity_access ea ON ea.owner_entity_id = e.id
LEFT JOIN entity_ports ep ON ep.entity_id = e.id AND ep.enabled = true
WHERE e.entity_type = 'server'
  AND e.deleted_at IS NULL
  AND (
    ea.subject_type = 'public_lite'
    OR (ea.subject_type = 'all_mine'        AND e.user_id = $client_user_id)
    OR (ea.subject_type = 'all_user_entities' AND ea.subject_user_id = $client_user_id)
    OR (ea.subject_type = 'entity'          AND ea.subject_entity_id = $client_entity_id)
  )
```

Also select the matching `ea.hostname` (the first non-null one for this client's access rule) so the generated command can use a friendly hostname where configured.

**Files to change (backend):**
- `crates/tunnel2tunnel-core/src/models/entity.rs` — add `Entity::list_reachable_for_client(pool, client_entity_id: Uuid, client_user_id: Uuid) -> Result<Vec<ReachableServer>>`
- `crates/tunnel2tunnel-web/src/routes/entities.rs` — add `list_reachable_servers` handler; 400 if entity is not a client
- `crates/tunnel2tunnel-web/src/lib.rs` — register `GET /api/entities/{entity_id}/reachable-servers`

**New response type** (in `tunnel2tunnel-web` or `tunnel2tunnel-core`):
```rust
pub struct ReachableServer {
    pub entity: Entity,        // the server entity
    pub ports: Vec<EntityPort>, // its enabled ports
    pub hostname: Option<String>, // from matching entity_access.hostname
}
```

### 2. Frontend API — `api/entities.ts`

Add interface and fetch function:
```typescript
export interface ReachableServer {
  entity: Entity
  ports: EntityPort[]
  hostname: string | null
}

export async function getReachableServers(entityId: string): Promise<ReachableServer[]>
```

### 3. Frontend page — `EntityDetailPage.vue`

After loading the entity, if `entity.entity_type === 'client'`, call `getReachableServers(entityId)` and store the result in a `ref<ReachableServer[]>`. Pass it to `SshCommandDisplay`.

### 4. Frontend component — `SshCommandDisplay.vue`

**New prop:** `reachableServers?: ReachableServer[]`

**New local state:** `acceptedPorts: Record<string, boolean>` keyed by `${serverId}/${portId}`, all defaulting to `true`.

**New UI section** (client entities only, below or integrated with the existing command box):

```
Authorized server ports
┌────────────────────────────────────────────────────┐
│ ☑ server-name (or uuid if no name)                │
│   ☑  3306 → proxy 5000   MySQL                    │
│   ☑  5432 → proxy 5001   PostgreSQL               │
│                                                    │
│ ☑ other-server                                    │
│   ☑  80   → proxy 5002   HTTP                    │
└────────────────────────────────────────────────────┘
```

Per-server checkbox acts as "select all ports for this server" (indeterminate if partial). Per-port checkbox toggles individual `-L` flags.

**SSH command generation** — for client entities, append to the command:
```
-L {port.local_port}:{hostname ?? server.id}:{port.proxy_port}
```
for each accepted server port. The client's own entity ports (current behaviour) remain unchanged and appear first.

---

## Verification

1. Start the dev stack (`cargo run -p t2t` + `npm run dev`)
2. Create a server entity with 2 enabled ports and an `all_mine` access rule
3. Create a client entity owned by the same user
4. Open the client detail page — the server's 2 ports should appear with checkboxes, both checked
5. The SSH command shows `-L` flags for both ports with the server UUID
6. Uncheck one port → command updates immediately (that `-L` disappears)
7. Server with only `entity`-type access rule for a different client → should NOT appear for our client
8. Server with `public_lite` access → should appear for any client
