Excellent! Now I have all the information I need. Let me create a comprehensive summary of my findings.

## Summary

I've thoroughly explored the tunnel2tunnel frontend codebase and its backend integration. Here are my findings:

### 1. **SshCommandDisplay Component** (`/home/user/git/luckydonald/tunnel2tunnel/frontend/src/components/SshCommandDisplay.vue`)

**Props:**
- `entity: Entity` - the entity (client or server)
- `ports: EntityPort[]` - array of port configurations
- `filename?: string` - SSH key filename (default: `'t2t_key'`)
- `t2tHost?: string` - t2t server hostname (default: `window.location.hostname`)
- `t2tSshPort?: number` - t2t SSH port (default: `2222`)

**SSH Command Generation:**
The component generates a command with different port flags based on entity type:

```typescript
function portFlag(port: EntityPort): string {
  return props.entity.entity_type === 'server'
    ? `-R ${port.proxy_port}:localhost:${port.local_port}`
    : `-L ${port.local_port}:<server>:${port.proxy_port}`
}
```

**What It Shows:**
- For **servers**: `-R` (remote forward) flags mapping `proxy_port` to `local_port`
- For **clients**: `-L` (local forward) flags mapping `local_port` to `proxy_port`, with `<server>` as a placeholder
- Only **enabled ports** are included in the SSH command (`enabledPorts` computed property filters by `p.enabled`)
- A ports table showing port details with different column headers based on entity type:
  - Servers: "Proxy port" | "Local port"
  - Clients: "Local port" | "Server proxy port"
- A hint for clients: "Replace `<server>` with the server entity ID"

### 2. **EntityDetailPage** (`/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/EntityDetailPage.vue`)

**How It Uses SshCommandDisplay:**
```html
<SshCommandDisplay :entity="entity" :ports="entity.ports" :filename="keyFilename" />
```

**Data Loading:**
- Loads `EntityDetail` on mount via `entitiesApi.getEntity(entityId)` (line 46)
- `EntityDetail` extends `Entity` and includes `ssh_keys: SshKey[]` and `ports: EntityPort[]`

**Client vs Server Distinction:**
The page distinguishes entity type in several places:
- Line 285-286: Breadcrumb navigation uses `entity.entity_type === 'server'` to link to `/servers` or `/clients`
- The distinction determines:
  - Which section tab to return to
  - How to display and interpret port mappings in the SSH command

**Sections Displayed:**
1. **SSH Command** - via SshCommandDisplay (shows ports)
2. **Ports** - edit/manage ports with enable/disable checkboxes
3. **SSH Keys** - list existing keys and add new ones
4. **Access Rules** - define who can access this entity (via `friendsApi.listAccess(entityId)`)
5. **Connection Log** - connection attempts and results

### 3. **entities.ts API** (`/home/user/git/luckydonald/tunnel2tunnel/frontend/src/api/entities.ts`)

**Key Interfaces:**
```typescript
interface Entity {
  id: string
  entity_type: string  // 'server' or 'client'
  name: string | null
  description: string | null
  ip_whitelist: string | null
  valid_until: string | null
  created_at: string
  updated_at: string
  deleted_at: string | null
}

interface EntityPort {
  id: string
  entity_id: string
  enabled: boolean
  local_port: number
  proxy_port: number
  name: string | null
  description: string | null
  sort_order: number
  created_at: string
  updated_at: string
}

interface EntityDetail extends Entity {
  ssh_keys: SshKey[]
  ports: EntityPort[]
}
```

**Data Available:**
- Ports are always loaded with the entity detail (line 237 in backend: `EntityPort::list_for_entity(&state.db, id)`)
- All ports are included, regardless of `enabled` status; filtering happens in frontend (SshCommandDisplay's `enabledPorts` computed property)

**Access Rules API** (via `friends.ts`):
```typescript
interface AccessRule {
  id: string
  owner_entity_id: string
  subject_type: 'entity' | 'all_mine' | 'all_user_entities' | 'public_lite'
  subject_entity_id: string | null
  subject_user_id: string | null
  hostname: string | null
  created_at: string
  updated_at: string
}
```

The access rules define **who can SSH into an entity**:
- `public_lite`: Anyone without an account
- `all_mine`: All entities owned by the same user (used internally)
- `all_user_entities`: All entities belonging to a specific friend's user account
- `entity`: A specific entity (by ID)

### 4. **entities Store** (`/home/user/git/luckydonald/tunnel2tunnel/frontend/src/stores/entities.ts`)

**State:**
```typescript
const entities = ref<Entity[]>([])  // List of entities
const loading = ref(false)
const error = ref<string | null>(null)
```

**Methods:**
- `fetchEntities(entityType?: 'server' | 'client')` - loads entities, optionally filtered by type
- `createEntity(params)` - creates new entity and prepends to list
- `deleteEntity(id)` - deletes entity and removes from list

**Note:** This store is used for **list pages** (EntitiesPage). The **detail page** (EntityDetailPage) loads full entity data directly via `entitiesApi.getEntity()` and manages state locally with `ref<EntityDetail>`.

### Key Insights

**What Ports Data Is Currently Shown to Client Entities:**
- All **enabled** ports are displayed in the SSH command section
- For clients: the command shows `-L ${port.local_port}:<server>:${port.proxy_port}` for each enabled port
- The `<server>` placeholder must be replaced with a target server entity ID when running the command

**SSH Command Generation Flow:**
1. `EntityDetailPage` loads `EntityDetail` (includes all ports)
2. Passes `entity` and `ports` to `SshCommandDisplay`
3. Component filters `ports` to `enabledPorts` via computed property
4. For each enabled port, generates appropriate flag (`-R` for server, `-L` for client)
5. Renders command with highlighted flags and a ports table

The ports in the SSH command are **strictly derived from** the `EntityPort` records returned by the backend—there's currently **no filtering or access-rule-based conditioning** of which ports appear in the SSH command.