# Plan: tunnel2tunnel — SSH Tunnel Manager

## Context

Implementing `tunnel2tunnel` from scratch per the spec in `init.md`. The app is a self-hosted SSH rendezvous server: "servers" and "clients" (both SSH keys registered in the system) can establish encrypted SSH tunnels that are routed through the t2t node, breaking through firewalls without exposing ports publicly. A web UI manages keys, permissions, users, and shows audit logs.

**Stack:**
- Backend: Rust (Axum + SQLx async)
- Frontend: Vue 3 + Vite
- Database: PostgreSQL
- SSH layer: `russh` crate (pure-Rust SSH server embedded in the binary)
- Auth: server-side sessions (tower-sessions + PostgreSQL store)
- Deployment: Docker Compose (standard + Coolify variant)

---

## Implementation Phases

### Phase 1 — Project Scaffold + Auth

**Goal:** Running binary with login/logout, admin bootstrapped from env vars.

**Files to create:**
- `Cargo.toml` (workspace)
- `crates/t2t-core/` — shared DB types, models, error types
- `crates/t2t-web/` — Axum HTTP server, routes, session middleware
- `crates/t2t-ssh/` — russh SSH server (stub at first)
- `crates/t2t-server/` — main binary that starts both servers
- `migrations/` — SQLx migration files
- `frontend/` — Vite + Vue 3 project (`npm create vite@latest`)
- `docker-compose.yml`, `docker-compose.coolify.yml`, `.env.example`

**DB schema (migration 001):**
```sql
users (id UUID PK, username TEXT UNIQUE, email TEXT, password_hash TEXT,
       is_admin BOOL, is_locked BOOL, created_at, updated_at, deleted_at)
settings (key TEXT PK, value TEXT)  -- e.g. signup_enabled
```

**Env vars at start:**
- `ADMIN_USERNAME`, `ADMIN_PASSWORD` — bootstrap admin (only on first run if user doesn't exist)
- `DATABASE_URL`, `SSH_PORT` (default 2222), `HTTP_PORT` (default 3000)
- `SIGNUP_ENABLED` (default false)

**Routes:**
- `GET /` → redirect to `/dashboard` or `/login`
- `GET/POST /login`, `POST /logout`
- `GET /dashboard` (stub)

**Vue pages:** Login form, Dashboard shell with sidebar nav.

---

### Phase 2 — Entity (Server/Client) Management

**Goal:** Full CRUD for entities with pubkey upload and IP whitelist.

**DB schema (migration 002):**
```sql
entities (id UUID PK, user_id UUID FK, type TEXT CHECK(type IN ('server','client')),
          name TEXT, description TEXT, pubkey TEXT,
          ip_whitelist TEXT,  -- newline-separated rules
          valid_until TIMESTAMPTZ,
          created_at, updated_at, deleted_at)
```

**API routes (JSON, require session):**
- `GET/POST /api/entities` — list (filtered by type query param) / create
- `GET/PUT/DELETE /api/entities/:id`

**Pubkey upload handling:**
- Parse `authorized_keys` line format: `<algo> <b64key> <comment>`
- Pre-fill name from filename, description from comment if fields empty

**Vue pages:**
- `/servers` and `/clients` — table with New/Edit/Delete
- Entity form modal: file input for pubkey (auto-fills name/description), name, description, ip_whitelist textarea, valid_until date picker
- `/entities` — combined view with type icon

---

### Phase 3 — Access Control + Friend System

**Goal:** Define which entities can connect to which; cross-user sharing.

**DB schema (migration 003):**
```sql
entity_access (
  id UUID PK,
  owner_entity_id UUID FK,   -- the entity being accessed
  subject_type TEXT,          -- 'entity'|'all_mine'|'all_user_entities'|'public_lite'
  subject_entity_id UUID FK nullable,
  subject_user_id UUID FK nullable,
  hostname TEXT nullable,     -- optional alias for SSH command
  created_at, updated_at
)

friendships (
  id UUID PK,
  from_user_id UUID FK,
  to_user_id UUID FK,
  status TEXT CHECK(status IN ('pending','accepted','declined')),
  -- what from_user allows to_user to see
  can_see_clients BOOL, can_see_servers BOOL, can_see_all BOOL,
  created_at, updated_at
)
```

**API routes:**
- `GET/POST /api/entities/:id/access` — list / add access rule
- `DELETE /api/entities/:id/access/:rule_id`
- `GET/POST /api/friends` — list / send invite
- `PUT /api/friends/:id` — accept/decline

**Vue pages:**
- Entity detail: "Clients" / "Servers" tab — checkboxes, select-all/none, "all mine" option, friend entities
- `/friends` — pending invites at top, friend cards with shared resource summary, quick-share button
- User lookup by exact username for friend invite

---

### Phase 4 — SSH Server (russh)

**Goal:** Functional SSH tunnel routing between registered entities.

**russh integration (`crates/t2t-ssh/`):**
- Implement `russh::server::Handler` trait
- Auth: `auth_publickey` — look up key fingerprint in `entities` table, check `valid_until`, check IP whitelist against peer addr, check entity_access rules
- Channel: `channel_open_direct_tcpip` — route data between the two connected SSH sessions
- Store active sessions in shared `Arc<Mutex<HashMap<EntityId, ActiveSession>>>`

**Routing logic:**
- Client opens SSH tunnel to t2t: `ssh -L localport:target-hostname:targetport user@t2t`
- `target-hostname` is looked up in `entity_access.hostname` to resolve target entity
- If a server entity is connected with a matching forwarded port, pipe the two TCP streams together
- Log connection start/end + key used + ports + IPs to `connection_logs`

**DB schema (migration 004):**
```sql
connection_logs (
  id UUID PK, entity_id UUID FK, peer_ip INET,
  key_fingerprint TEXT, ssh_flags TEXT, ports_requested TEXT,
  login_succeeded BOOL, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ
)
```

---

### Phase 5 — Admin Panel, Settings, Audit Log UI

**Goal:** Complete the remaining UX flows from spec.

**Admin routes (require `is_admin`):**
- `GET /api/admin/users` — list all users
- `POST /api/admin/users` — create user
- `PUT /api/admin/users/:id` — edit (including password reset, admin toggle)
- Atomic demote: check `count(*) WHERE is_admin = true > 1` in same transaction

**User settings routes:**
- `PUT /api/me/password` — old + new + confirm; env var does NOT override after init
- `GET /api/me/keys` — all entity keys owned by user (for purge page)
- `POST /api/me/purge` — delete selected keys / revoke access rules

**Vue pages:**
- `/admin/users` — table, edit modal, admin toggle (danger-styled self-demote), "New User" form, signup toggle
- `/settings` — change password form, links to key purge + access purge pages
- `/settings/purge-keys` — checkboxes for all server/client keys, confirm step, cancel retains selection
- `/settings/purge-access` — same for entity_access rows
- Entity detail page — connection log table (last 10), SSH command snippet

**SSH command generation:**
```
ssh -R <port>:localhost:<local_port> <entity_id>@<t2t_host> -p <ssh_port>  # for servers
ssh -L <local_port>:<target_hostname>:<port> <entity_id>@<t2t_host> -p <ssh_port>  # for clients
```

---

## Key Technical Decisions

| Concern | Decision |
|---|---|
| Session store | `tower-sessions` + `tower-sessions-sqlx-store` (Postgres) |
| Password hashing | `argon2` crate |
| SSH key parsing | `ssh-key` crate (parses authorized_keys format, gives fingerprint) |
| IP whitelist matching | Custom parser: CIDR (`ipnetwork`), glob (`glob` crate), regex (`regex` crate), `!` prefix for deny |
| Frontend state | Pinia stores per domain (auth, entities, friends) |
| API auth | Session cookie; 401 → Vue router redirects to /login |
| Soft delete | `deleted_at IS NULL` filter on all queries; `DELETE /api/entities/:id` sets `deleted_at = now()` |
| Self-demote guard | `SELECT COUNT(*) FROM users WHERE is_admin = true FOR UPDATE` in transaction before demote |

---

## Cargo Dependencies

```toml
# crates/t2t-core
sqlx = { features = ["postgres", "uuid", "time", "runtime-tokio"] }
uuid = { features = ["v4"] }
time = "0.3"
argon2 = "0.5"
ssh-key = "0.6"
ipnetwork = "0.20"
regex = "1"
glob = "0.3"

# crates/t2t-web  
axum = { features = ["multipart"] }
tower-sessions = "0.13"
tower-sessions-sqlx-store = { features = ["postgres"] }
serde = { features = ["derive"] }
serde_json = "1"

# crates/t2t-ssh
russh = "0.44"
russh-keys = "0.44"
tokio = { features = ["full"] }
```

---

## Verification

1. `cargo build` — clean compile, no warnings
2. `docker compose up` — Postgres starts, migrations run, admin user bootstrapped
3. Browse `http://localhost:3000` → redirects to `/login`
4. Login as admin, navigate all pages (servers, clients, entities, friends, admin/users, settings)
5. Upload a pubkey file — name/description auto-filled from filename/comment
6. Create entity access rule, verify SSH command shown on detail page
7. `ssh -p 2222 <entity_id>@localhost` with matching private key → connection logged
8. Two SSH sessions (server + client with valid access rule) → data flows end-to-end
9. Self-demote blocked when last admin; confirm dialog shown
10. Purge page: cancel returns to same selection state

---

## File Layout

```
tunnel2tunnel/
├── Cargo.toml                  # workspace
├── crates/
│   ├── t2t-core/src/           # models, db queries, error
│   ├── t2t-web/src/            # axum routes, handlers, middleware
│   ├── t2t-ssh/src/            # russh server, tunnel router
│   └── t2t-server/src/main.rs  # tokio::main, starts both
├── migrations/
│   ├── 001_users.sql
│   ├── 002_entities.sql
│   ├── 003_access.sql
│   └── 004_connection_logs.sql
├── frontend/
│   ├── src/
│   │   ├── pages/              # Login, Dashboard, Servers, Clients,
│   │   │   │                   # Entities, Friends, Admin/Users, Settings,
│   │   │   │                   # PurgeKeys, PurgeAccess, EntityDetail
│   │   ├── stores/             # Pinia: auth, entities, friends, admin
│   │   ├── components/         # EntityForm, AccessMatrix, FriendCard, ...
│   │   └── router/index.ts
│   └── vite.config.ts
├── docker-compose.yml
├── docker-compose.coolify.yml
└── .env.example
```
