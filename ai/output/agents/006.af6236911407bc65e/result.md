Excellent! Now I have a complete understanding. Let me provide you with a comprehensive analysis.

## Comprehensive Backend Analysis: tunnel2tunnel

### 1. Entity Ports Database Schema

**File:** `/home/user/git/luckydonald/tunnel2tunnel/migrations/002_entities_keys_ports.sql`

The `entity_ports` table has the following fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID | Primary key, auto-generated (uuidv7) |
| `entity_id` | UUID | Foreign key to `entities(id)` |
| `enabled` | BOOL | Whether this port forward is active (default: true) |
| `local_port` | INTEGER | Port on the server's local service (source) |
| `proxy_port` | INTEGER | Port on the SSH tunnel (destination) |
| `name` | TEXT | Optional friendly name |
| `description` | TEXT | Optional description |
| `sort_order` | INTEGER | Ordering value for UI (default: 0) |
| `created_at` | TIMESTAMPTZ | Creation timestamp |
| `updated_at` | TIMESTAMPTZ | Last update timestamp |

The table has an automatic timestamp trigger for `created_at` and `updated_at`.

---

### 2. Ports API Routes

**File:** `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/lib.rs` (lines 60-66)

```
GET    /api/entities/{entity_id}/ports          → list_ports
POST   /api/entities/{entity_id}/ports          → create_port
PUT    /api/entities/{entity_id}/ports/{port_id} → update_port
DELETE /api/entities/{entity_id}/ports/{port_id} → delete_port
```

**Data Returned (GET response):** `EntityPortResponse` containing:
```json
{
  "id": "uuid",
  "entity_id": "uuid",
  "enabled": true,
  "local_port": 3306,
  "proxy_port": 5432,
  "name": "string or null",
  "description": "string or null",
  "sort_order": 0,
  "created_at": "2026-06-30T12:00:00Z",
  "updated_at": "2026-06-30T12:00:00Z"
}
```

**POST/PUT Request bodies:**
- POST accepts: `enabled` (optional), `local_port`, `proxy_port`, `name`, `description`, `sort_order` (optional)
- PUT requires: `enabled`, `local_port`, `proxy_port`, `name`, `description`, `sort_order` (all fields)

Routes are defined in `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/entities.rs` (lines 314-379).

---

### 3. Entity Access Table

**File:** `/home/user/git/luckydonald/tunnel2tunnel/migrations/003_access_friends.sql` (lines 1-15)

The `entity_access` table fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID | Primary key, auto-generated (uuidv7) |
| `owner_entity_id` | UUID | Foreign key to `entities(id)` - the server being accessed |
| `subject_type` | TEXT | Access rule type (one of: `'entity'`, `'all_mine'`, `'all_user_entities'`, `'public_lite'`) |
| `subject_entity_id` | UUID | (Optional) FK to `entities(id)` - specific client entity when `subject_type='entity'` |
| `subject_user_id` | UUID | (Optional) FK to `users(id)` - specific user when `subject_type='all_user_entities'` |
| `hostname` | TEXT | (Optional) Alias hostname for connecting to this entity via SSH tunnel |
| `created_at` | TIMESTAMPTZ | Creation timestamp |
| `updated_at` | TIMESTAMPTZ | Last update timestamp |

**Subject types explain:**
- `'entity'`: Only a specific entity (via `subject_entity_id`) can access
- `'all_mine'`: Only entities owned by the same user as the owner entity can access
- `'all_user_entities'`: All entities owned by a specific user (via `subject_user_id`) can access
- `'public_lite'`: Public access (anyone can access)

**Important:** Port forwarding configuration is NOT stored in `entity_access`. Port forwarding is entirely defined per entity via the `entity_ports` table. The `entity_access` table only controls WHO can access an entity, not WHICH ports they can access.

---

### 4. Entity Access API Routes

**File:** `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/lib.rs` (lines 68-72)

```
GET    /api/entities/{entity_id}/access         → list_access
POST   /api/entities/{entity_id}/access         → create_access
DELETE /api/entities/{entity_id}/access/{rule_id} → delete_access
```

**GET response:** `AccessResponse` containing:
```json
{
  "id": "uuid",
  "owner_entity_id": "uuid",
  "subject_type": "entity|all_mine|all_user_entities|public_lite",
  "subject_entity_id": "uuid or null",
  "subject_user_id": "uuid or null",
  "hostname": "example.tunnel or null",
  "created_at": "2026-06-30T12:00:00Z",
  "updated_at": "2026-06-30T12:00:00Z"
}
```

**POST request body:**
```json
{
  "subject_type": "entity|all_mine|all_user_entities|public_lite",
  "subject_entity_id": "uuid or null",
  "subject_user_id": "uuid or null",
  "hostname": "optional alias"
}
```

Routes implemented in `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/access.rs`.

---

### 5. SSH Tunnel Port Forwarding Logic

**File:** `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs`

The SSH server implements a sophisticated SSH tunnel bridging system:

#### Authentication Flow (lines 180-260):
1. Client connects with SSH public key
2. Key fingerprint validated against database via `SshKey::find_by_fingerprint()`
3. Key expiry checked (`valid_until` field)
4. Entity expiry checked (`valid_until` field on entity)
5. IP whitelist enforcement (`entity.ip_whitelist`)
6. Connection logged with `ConnectionLog::create()`

#### Port Registration - Server Side (lines 263-279):
When a **server** entity registers a remote port (-R proxy_port:localhost:local_port):
```rust
async fn tcpip_forward(&mut self, _address: &str, port: &mut u32, session: &mut Session)
```
- The port is registered in `server_slots: HashMap<(entity_id, proxy_port), Handle>`
- Logs: `"server registered port"`
- Stores the SSH session handle for this entity+port combination

#### Port Forwarding - Client Side (lines 298-390):
When a **client** entity opens a direct-tcpip channel (-L local:hostname:proxy_port):
```rust
async fn channel_open_direct_tcpip(&mut self, channel, host_to_connect, port_to_connect, ...)
```

**The forwarding flow:**
1. Resolves target hostname to entity ID:
   - Tries parsing as UUID first
   - Falls back to `EntityAccess::find_entity_by_hostname()` lookup
2. **Access control check:**
   ```rust
   EntityAccess::check_access(
       pool,
       target_entity_id,      // server entity being accessed
       client_entity.id,      // client entity making request
       client_user_id,        // owner of client
       target_entity.user_id  // owner of server
   )
   ```
3. Looks up registered server handle in `server_slots` by `(entity_id, proxy_port)`
4. Opens a forwarded-tcpip channel on the server
5. **Bridges the channels:** data flows:
   - Client channel ↔ Server channel (lines 392-403)
   - Bidirectional copy with EOF/close propagation

#### Access Check Logic (lines 96-123 in entity_access.rs):
```rust
pub async fn check_access(
    target_entity_id,
    client_entity_id,
    client_user_id,
    target_user_id,
) -> bool
```
Returns true if ANY of these conditions are met:
- `subject_type = 'public_lite'` (public access)
- `subject_type = 'all_mine' AND client_user_id == target_user_id` (same user)
- `subject_type = 'all_user_entities' AND subject_user_id == client_user_id`
- `subject_type = 'entity' AND subject_entity_id == client_entity_id`

#### Hostname Resolution (lines 456-465):
`EntityAccess::find_entity_by_hostname()` queries:
```sql
SELECT owner_entity_id FROM entity_access WHERE hostname = $1 LIMIT 1
```
This allows users to connect via friendly names instead of UUIDs.

---

### 6. Key Integration Points

**Database Models:**
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/entity_port.rs` - CRUD operations
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/entity_access.rs` - Access rule checking
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/entity.rs` - Entity validation (expiry, IP whitelist)

**Connection Logging:**
- Successful and failed SSH authentications logged to `connection_logs` table
- Failed2ban integration for brute-force protection
- Connection end timestamps recorded on disconnect

**Data Flow:**
```
SSH Client → SSH Server (auth via key + entity_access) → 
  Register port (-R) / Open tunnel (-L) → 
  Access check (EntityAccess::check_access) → 
  Channel bridge → Data forwarding
```

---

### Summary Table

| Component | Location | Purpose |
|-----------|----------|---------|
| Schema | `/migrations/002_entities_keys_ports.sql` | Port configuration storage |
| Schema | `/migrations/003_access_friends.sql` | Access control rules |
| API Routes | `/crates/tunnel2tunnel-web/src/routes/entities.rs` | Port CRUD endpoints |
| API Routes | `/crates/tunnel2tunnel-web/src/routes/access.rs` | Access rule endpoints |
| SSH Handler | `/crates/tunnel2tunnel-ssh/src/lib.rs` | Port forwarding + bridging |
| Models | `/crates/tunnel2tunnel-core/src/models/` | Database abstraction layer |

The system separates **port configuration** (entity_ports table) from **access control** (entity_access table), allowing granular control over which entities can access which ports on a given server.