# Plan: tunnel2tunnel — SSH Tunnel Manager

## Context

Implementing `tunnel2tunnel` from scratch per the spec in `init.md`. A self-hosted SSH rendezvous server: "servers" and "clients" register SSH keys, then establish encrypted tunnels routed through the tunnel2tunnel node, breaking firewalls without exposing ports publicly. A Vue 3 SPA manages keys, permissions, users, and audit logs.

**Stack:**
- Backend: Rust (Axum + SQLx async), workspace with multiple crates, binary called `t2t`
- Frontend: Vue 3 (Composition API, `<script setup>`, fully-typed TS — no `any`/`unknown`, SCSS)
- Database: PostgreSQL, all timestamps `TIMESTAMPTZ`, server/DB in UTC
- SSH layer: `russh` crate (pure-Rust SSH server embedded in the binary)
- Auth: server-side sessions (`tower-sessions` + `tower-sessions-sqlx-store`)
- IDs: UUIDv7 everywhere (`uuid` crate with v7 feature)
- Deployment: Docker Compose (standard + Coolify variant)

---

## Clarifications

### Friendship vs. Entity Access (these are two distinct things)

**Friendship** — controls *visibility*: whether another user can see your entities at all.
When user A and user B are friends, A can grant B visibility of A's entities. Without that grant,
B cannot even discover A's entities to reference them in an access rule. This is needed to enable
cross-user access (step 13.4 in spec). The `visibility_grant` field records what A exposes to B.

**Entity Access** — controls *connectivity*: whether a specific entity (server or client) can
establish a tunnel through another entity. This lives in `entity_access`, totally separate from
friendship. You can grant access to your own entities freely; you can only grant access to a
friend's entities if they've made those entities visible to you.

### SSH Keys separated from Entities

Entities no longer have an embedded `pubkey` field. Instead an `ssh_keys` table holds the
keys, linked to an entity via FK. An entity can have multiple keys (e.g. rotated key); the
purge-keys page operates on `ssh_keys` rows directly.

---

## Phase 0 — Feasibility: Minimal russh Tunnel

**Goal:** Prove `russh` can pipe two SSH sessions together. No web, no DB, no auth.

**What it does:**
- Single Rust binary, listens on an SSH port (e.g. 2222)
- Accepts all incoming SSH connections with no auth (accept any key or password)
- Slots: `server` and `client` (one connection each)
- When a "server" connects and opens a forwarded-tcpip channel, hold it
- When a "client" connects and opens a direct-tcpip channel for the same target, splice
  the two byte streams together
- Print connect/disconnect events to stdout
- Connection strings to test: `ssh -R 8080:localhost:8080 server@localhost -p 2222` and
  `ssh -L 9090:server:8080 client@localhost -p 2222`

**Files:**
- `crates/t2t-feasibility/src/main.rs` — standalone, throwaway once Phase 4 subsumes it
- **No migrations, no web, no Vue**

---

## Phase 1 — Project Scaffold + Auth

**Goal:** Running binary, login/logout, admin bootstrapped from env vars.

### Cargo workspace

```
tunnel2tunnel/
├── Cargo.toml          # workspace
└── crates/
    ├── tunnel2tunnel-core/    # DB models, queries, shared types
    ├── tunnel2tunnel-web/     # Axum HTTP server + API routes
    ├── tunnel2tunnel-ssh/     # russh SSH server + tunnel router
    └── t2t/                   # main binary (joins all three)
```

### DRY timestamps — DB side

Define reusable PostgreSQL helpers in migration 000:
```sql
-- uuidv7() is built-in as of PostgreSQL 18 — no extension needed.

-- Auto-update trigger for created_at / updated_at
CREATE OR REPLACE FUNCTION set_timestamps()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    NEW.created_at := NOW();
  END IF;
  NEW.updated_at := NOW();
  RETURN NEW;
END;
$$;
```

`docker-compose.yml` uses `postgres:18`. No extension needed — `uuidv7()` is a built-in.

Every subsequent migration calls this function via a one-liner macro:
```sql
CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON <table>
  FOR EACH ROW EXECUTE FUNCTION set_timestamps();
```

### DRY timestamps — Rust side

`tunnel2tunnel-core` defines three composable structs that SQLx can flatten.
`TimestampsSoftDelete` composes the other two via nested `#[sqlx(flatten)]`:

```rust
#[derive(sqlx::FromRow, serde::Serialize)]
pub struct Timestamps {
    pub created_at: OffsetDateTime,   // set by trigger + DEFAULT NOW()
    pub updated_at: OffsetDateTime,   // updated by trigger
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct SoftDelete {
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct TimestampsSoftDelete {
    #[sqlx(flatten)]
    pub timestamps: Timestamps,
    #[sqlx(flatten)]
    pub soft_delete: SoftDelete,
}
```

Usage in models:
- Plain timestamps only: `#[sqlx(flatten)] pub ts: Timestamps`
- With soft-delete: `#[sqlx(flatten)] pub ts: TimestampsSoftDelete`

SQLx's `#[sqlx(flatten)]` resolves fields recursively, so nesting works.

### DB schema (migration 001 — users + settings)

```sql
CREATE TABLE users (
  id          UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  username    TEXT NOT NULL UNIQUE,
  email       TEXT,
  password_hash TEXT NOT NULL,
  is_admin    BOOL NOT NULL DEFAULT false,
  is_locked   BOOL NOT NULL DEFAULT false,
  description TEXT,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at  TIMESTAMPTZ
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- e.g. ('signup_enabled', 'false'), ('fail2ban_log_path', '')
```

### Env vars

- `ADMIN_USERNAME`, `ADMIN_PASSWORD` — bootstrap only if user does not exist yet
- `DATABASE_URL`, `SSH_PORT` (default 2222), `HTTP_PORT` (default 3000)
- `SIGNUP_ENABLED` (default false, seeds `settings` on boot, but DB value wins after first run)
- `FAIL2BAN_LOG_PATH` — if set, write auth events in sshd-compatible format to this file

### Routes (Phase 1)

- `GET /` → redirect `/dashboard` or `/login`
- `GET /login`, `POST /login`, `POST /logout`
- `GET /dashboard` (stub)

### Vue (Phase 1)

- `LoginPage.vue` — form, error state
- `AppShell.vue` — sidebar nav, router outlet
- Pinia `authStore` — current user, logout action
- Router guard: unauthenticated → `/login`

---

## Phase 2 — SSH Keys + Entities

**Goal:** Full CRUD for entities and their SSH keys; pubkey textarea + file upload.

### DB schema (migration 002)

```sql
CREATE TABLE entities (
  id          UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  user_id     UUID NOT NULL REFERENCES users(id),
  type        TEXT NOT NULL CHECK(type IN ('server', 'client')),
  name        TEXT,   -- nullable; UI shows UUID in <code> when null
  description TEXT,
  ip_whitelist TEXT,  -- newline-separated rules
  valid_until TIMESTAMPTZ,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at  TIMESTAMPTZ
);

CREATE TABLE ssh_keys (
  id          UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  entity_id   UUID NOT NULL REFERENCES entities(id),
  algorithm   TEXT NOT NULL,
  key_data    TEXT NOT NULL,   -- the base64 blob
  comment     TEXT,
  fingerprint TEXT NOT NULL UNIQUE,  -- SHA256:… for fast auth lookup
  name        TEXT,
  valid_until TIMESTAMPTZ,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at  TIMESTAMPTZ
);
```

### Entity ports (migration 002b)

```sql
CREATE TABLE entity_ports (
  id          UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  entity_id   UUID NOT NULL REFERENCES entities(id),
  enabled     BOOL NOT NULL DEFAULT true,
  local_port  INTEGER NOT NULL,   -- port on the server/client machine
  proxy_port  INTEGER NOT NULL,   -- port internal to tunnel2tunnel
  name        TEXT,
  description TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### Vue pubkey component

`PubkeyInput.vue`:
- Textarea (v-model the raw key text) + file button
- On file select: check MIME is text or extension is `.pub`/`.pem`/`.key`; reject if > 16 KB
- Parse `authorized_keys` format: split on whitespace → `[algo, b64, comment]`
- Emit parsed `{ algorithm, keyData, comment }` to parent
- Parent pre-fills entity name from filename (minus extension), description from comment

### `EntityName.vue` component

```vue
<EntityName :entity="entity" />
<!-- renders entity.name if set, otherwise <code>{{ entity.id }}</code> -->
```

Used everywhere an entity is referenced — tables, dropdowns, breadcrumbs.

### Pages & components (Phase 2)

- `EntitiesPage.vue` — accepts `:entityType` prop (`'server'|'client'|undefined`)
  - `/servers` and `/clients` are just `<EntitiesPage entityType="server" />` etc.
  - `/entities` passes no prop → shows all, interleaved, with type icon badge
- `EntityFormModal.vue` — create/edit, includes `PubkeyInput`, port table
- `EntityDetailPage.vue` — shows SSH command (interactive), ports table, connection log

### SSH command generator (interactive)

- Computed from entity ports where `enabled = true`
- Hovering a port row highlights the corresponding `-L`/`-R` flag in the command string
- Hovering a flag in the command highlights the matching row
- Ports editable inline in the table; command updates live

---

## Phase 3 — Access Control + Friend System

### DB schema (migration 003)

```sql
CREATE TABLE entity_access (
  id               UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  owner_entity_id  UUID NOT NULL REFERENCES entities(id),
  subject_type     TEXT NOT NULL CHECK(subject_type IN (
                     'entity', 'all_mine', 'all_user_entities', 'public_lite')),
  subject_entity_id UUID REFERENCES entities(id),  -- set when subject_type='entity'
  subject_user_id   UUID REFERENCES users(id),      -- set when subject_type='all_user_entities'
  hostname         TEXT,   -- optional alias; if null, entity UUID also works
  created_at       TIMESTAMPTZ NOT NULL,
  updated_at       TIMESTAMPTZ NOT NULL
);

CREATE TABLE friendships (
  id              UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  from_user_id    UUID NOT NULL REFERENCES users(id),
  to_user_id      UUID NOT NULL REFERENCES users(id),
  status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK(status IN ('pending', 'accepted', 'declined')),
  -- bulk visibility: what from_user broadly grants to_user to see
  -- OR-ed with per-entity rows in friendship_entity_grants
  visibility_grant TEXT NOT NULL DEFAULT 'none'
                    CHECK(visibility_grant IN ('none', 'clients', 'servers', 'all')),
  created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(from_user_id, to_user_id)
);

-- per-entity explicit visibility grants within a friendship
-- an entity is visible to the friend if:
--   friendships.visibility_grant covers its type  OR  a row exists here
CREATE TABLE friendship_entity_grants (
  id            UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  friendship_id UUID NOT NULL REFERENCES friendships(id) ON DELETE CASCADE,
  entity_id     UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(friendship_id, entity_id)
);
```

### Friendship vs. access rules in the UI

- **Friends page** (`/friends`): manage friendships; set broad `visibility_grant` OR expand to pick specific entities via `friendship_entity_grants` rows — both are OR-conditions, not AND
- **Entity → Access tab**: add access rules; when selecting "other user's entities", only entities that friend has made visible appear in the picker
- Friend card summary (from spec):
  ```
  43 clients · 1 server · no full access
  full access (123 clients · 2 servers) + 2 clients + all servers
  ```

### Routes (Phase 3)

- `GET/POST /api/entities/:id/access`
- `DELETE /api/entities/:id/access/:rule_id`
- `GET/POST /api/friends`
- `PUT /api/friends/:id` — accept/decline/update visibility

---

## Phase 4 — SSH Server (russh)

**Goal:** Full auth-gated tunnel routing; fail2ban-compatible logging.

### Auth flow

1. Peer connects → look up key fingerprint in `ssh_keys` (where `deleted_at IS NULL`)
2. Check `ssh_keys.valid_until` and `entities.valid_until` — reject if expired
3. Evaluate `entity.ip_whitelist` rules against peer IP (top-to-bottom, first match wins):
   - Plain IP / CIDR (`ipnetwork` crate)
   - Glob `?`/`*` (`glob` crate)
   - Regex `s/…/flags` (`regex` crate)
   - `!` prefix = deny
4. Check `entity_access` — does this entity have permission to connect?
5. On any failure: log event + write fail2ban line if `FAIL2BAN_LOG_PATH` is set
6. On success: admit and route

### fail2ban integration

Write auth events to a file in sshd-compatible format:
```
Jun 25 12:00:01 t2t sshd[1234]: Failed publickey for invalid user <fingerprint> from 1.2.3.4 port 54321 ssh2
Jun 25 12:00:02 t2t sshd[1234]: Accepted publickey for <entity_id> from 1.2.3.4 port 54322 ssh2
```

Provide `contrib/fail2ban/filter.d/tunnel2tunnel.conf` for web login failures.

### Routing

- Active sessions held in `Arc<Mutex<HashMap<Uuid, ActiveSession>>>` (entity_id → session)
- `target-hostname` in the SSH command is matched against:
  1. `entity_access.hostname` (friendly alias)
  2. Entity UUID directly (always valid if access rule permits)
- Server session opens a remote-forwarded channel → parked in map
- Client session opens a direct-tcpip channel for that hostname → spliced with server channel

### DB schema (migration 004)

```sql
CREATE TABLE connection_logs (
  id               UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
  entity_id        UUID REFERENCES entities(id),
  peer_ip          INET,
  key_fingerprint  TEXT,
  login_succeeded  BOOL NOT NULL,
  failure_reason   TEXT,
  ssh_flags        TEXT,
  ports_requested  TEXT,
  started_at       TIMESTAMPTZ NOT NULL,
  ended_at         TIMESTAMPTZ,
  created_at       TIMESTAMPTZ NOT NULL,
  updated_at       TIMESTAMPTZ NOT NULL
);
```

---

## Phase 5 — Admin Panel, Settings, Audit Log UI

### Admin routes (require `is_admin`)

- `GET /api/admin/users`, `POST /api/admin/users`
- `PUT /api/admin/users/:id` — edit any field, reset password, toggle admin
- Self-demote guard: `SELECT COUNT(*) FROM users WHERE is_admin = true FOR UPDATE` in TX;
  reject if ≤ 1

### User settings routes

- `PUT /api/me/password` — validates old password; env var does NOT re-apply after init
- `GET /api/me/ssh-keys` — all ssh_keys owned by user (via entity FK chain)
- `POST /api/me/purge-keys` — soft-delete selected ssh_keys
- `POST /api/me/purge-access` — delete selected entity_access rows

### Vue pages (Phase 5)

- `/admin/users` — table, edit modal, admin toggle (danger-styled for self), signup toggle,
  "New User" form, "+Friend" quick button pre-fills username
- `/settings` — change password; links to purge pages
- `/settings/purge-keys` — select all/none per type, confirm step, cancel retains selection
- `/settings/purge-access` — same for access rules
- Entity detail — connection log (last 10), login attempts shown (passwords shown as `••••••`),
  SSH command

### Audit log display

- Show all attempts (success + failure)
- Passwords are never stored; show `••••••` as a fixed placeholder for the credential field
- Each row: timestamp, IP, key fingerprint / username, result, ports

---

## Key Technical Decisions

| Concern | Decision |
|---|---|
| UUIDs | UUIDv7 end-to-end: `uuid` crate v7 in Rust; `uuidv7()` built-in in PostgreSQL 18 (no extension). All `DEFAULT` clauses use `uuidv7()`. Rust always generates and passes the ID explicitly; the DB default is a safety net. |
| Session store | `tower-sessions` + `tower-sessions-sqlx-store` (Postgres) |
| Password hashing | `argon2` crate |
| SSH key parsing | `ssh-key` crate — parses authorized_keys line, produces fingerprint |
| IP whitelist | Custom evaluator: CIDR (`ipnetwork`), glob (`glob`), regex (`regex`), `!` invert |
| Frontend chunks | Vite `manualChunks`: vendor (vue/pinia/router), pages per route, shared components |
| Soft delete | `deleted_at IS NULL` on all queries; DELETE endpoint sets `deleted_at = NOW()` |
| Entity name fallback | `EntityName.vue` component renders `<code>{{ id }}</code>` when `name` is null |
| Self-demote guard | Atomic TX with `SELECT … FOR UPDATE` count check |

---

## Cargo Dependencies

```toml
# tunnel2tunnel-core
sqlx = { features = ["postgres", "uuid", "time", "runtime-tokio"] }
uuid = { version = "1", features = ["v7"] }
time = "0.3"
argon2 = "0.5"
ssh-key = "0.6"
ipnetwork = "0.20"
regex = "1"
glob = "0.3"
serde = { features = ["derive"] }

# tunnel2tunnel-web
axum = { features = ["multipart"] }
tower-sessions = "0.13"
tower-sessions-sqlx-store = { features = ["postgres"] }
serde_json = "1"

# tunnel2tunnel-ssh
russh = "0.44"
russh-keys = "0.44"
tokio = { features = ["full"] }
```

---

## File Layout

```
tunnel2tunnel/
├── Cargo.toml                          # workspace
├── crates/
│   ├── t2t-feasibility/src/main.rs     # Phase 0 throw-away proof of concept
│   ├── tunnel2tunnel-core/src/         # models, DB queries, shared error
│   ├── tunnel2tunnel-web/src/          # axum routes, handlers, middleware
│   ├── tunnel2tunnel-ssh/src/          # russh server, tunnel router
│   └── t2t/src/main.rs                 # binary entry point
├── migrations/
│   ├── 000_helpers.sql                 # set_timestamps() trigger function
│   ├── 001_users.sql
│   ├── 002_entities_keys_ports.sql
│   ├── 003_access_friends.sql
│   └── 004_connection_logs.sql
├── frontend/
│   ├── src/
│   │   ├── pages/                      # LoginPage, Dashboard, EntitiesPage,
│   │   │                               # EntityDetailPage, FriendsPage,
│   │   │                               # AdminUsersPage, SettingsPage,
│   │   │                               # PurgeKeysPage, PurgeAccessPage
│   │   ├── stores/                     # auth, entities, friends, admin (Pinia)
│   │   ├── components/
│   │   │   ├── EntityName.vue          # name || <code>uuid</code>
│   │   │   ├── PubkeyInput.vue         # textarea + file input
│   │   │   ├── SshCommandDisplay.vue   # interactive command + port table
│   │   │   └── ...
│   │   └── router/index.ts
│   └── vite.config.ts                  # manualChunks for sane splitting
├── contrib/
│   └── fail2ban/filter.d/tunnel2tunnel.conf
├── docker-compose.yml
├── docker-compose.coolify.yml
└── .env.example
```

---

## Unit Tests

Tests live alongside the code they cover (`#[cfg(test)]` modules in each crate) plus an integration test crate per crate under `tests/`.

### `tunnel2tunnel-core`

| Test | What it covers |
|---|---|
| `ip_whitelist::plain_ip_match` | exact IP matches |
| `ip_whitelist::cidr_match` | `192.168.1.0/24` accepts `.1`–`.254` |
| `ip_whitelist::glob_match` | `10.0.?.?` wildcard |
| `ip_whitelist::regex_match` | `s/^10\.0\..*/` |
| `ip_whitelist::inversion` | `!192.168.1.5` denies that IP |
| `ip_whitelist::order` — first match wins | deny-all with one allow exception |
| `pubkey::parse_authorized_keys_line` | algo + b64 + comment split correctly |
| `pubkey::fingerprint_stable` | same key → same SHA256 fingerprint |
| `pubkey::rejects_oversized` | > 16 KB key data → error |
| `timestamps::flatten_roundtrip` | `TimestampsSoftDelete` nested flatten survives sqlx encode/decode |
| `timestamps::softdelete_alone` | `SoftDelete` flatten works without `Timestamps` wrapper |

### `tunnel2tunnel-web`

Use `axum::test` helpers (no real Postgres — mock via `sqlx::test` or in-memory SQLite for unit scope, real Postgres for integration).

| Test | What it covers |
|---|---|
| `auth::login_success` | valid credentials → session cookie set |
| `auth::login_wrong_password` | 401, no cookie |
| `auth::login_locked_user` | 403 |
| `auth::unauthenticated_redirect` | protected route → 302 /login |
| `auth::bootstrap_idempotent` | calling bootstrap twice doesn't duplicate admin |
| `admin::demote_self_last_admin` | 409 when only one admin |
| `admin::demote_self_ok` | succeeds when two admins exist |
| `entities::create_name_null` | entity without name is valid |
| `entities::soft_delete` | GET after DELETE returns 404 |
| `password::old_password_required` | wrong old password → 400 |

### `tunnel2tunnel-ssh`

| Test | What it covers |
|---|---|
| `auth::fingerprint_lookup_miss` | unknown key → `Disconnect` |
| `auth::key_expired` | past `valid_until` → rejected |
| `auth::ip_whitelist_deny` | blocked IP → rejected |
| `auth::access_denied` | no `entity_access` row → rejected |
| `routing::hostname_alias_resolves` | `entity_access.hostname` maps to entity |
| `routing::uuid_resolves` | raw UUID as hostname works |
| `fail2ban::writes_failure_line` | failure event → correct sshd-format line in file |
| `fail2ban::writes_success_line` | success event → correct line |

### Frontend (Vitest + Vue Test Utils)

| Test | What it covers |
|---|---|
| `EntityName` | renders name when set; renders `<code>uuid</code>` when null |
| `PubkeyInput` | file > 16 KB → emits error; valid `.pub` → emits parsed parts |
| `PubkeyInput` | non-text file → emits error |
| `SshCommandDisplay` | hover port row → correct flag highlighted; edit port → command updates |
| `authStore` | unauthenticated state redirects router |
| `router` | all named routes resolve without 404 |

### Running tests

```bash
cargo test --workspace              # all Rust unit + integration tests
cd frontend && npx vitest run       # Vue component tests
```

---

## Verification

1. **Phase 0**: two `ssh` processes, one with `-R`, one with `-L` — data flows end-to-end
2. `cargo build --workspace` — clean, no warnings
3. `docker compose up` — Postgres starts, migrations apply, admin bootstrapped from env
4. Browse `http://localhost:3000` → `/login`; login as admin
5. Upload a `.pub` file — name/description pre-filled from filename/comment
6. Inline port table edits update SSH command string live; hover cross-highlights
7. Create entity access rule; SSH in with matching key → logged; wrong key → logged as failure
8. `FAIL2BAN_LOG_PATH=/tmp/auth.log t2t` → file written in sshd format on auth events
9. Self-demote blocked when last admin (atomic check)
10. Purge page cancel → selection retained; confirm → keys soft-deleted
