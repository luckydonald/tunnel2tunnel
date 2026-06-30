# CLAUDE.md — tunnel2tunnel

Self-hosted SSH rendezvous manager. Servers and clients register SSH public keys and establish encrypted tunnels through this node without exposing ports publicly.

---

## Architecture

```
tunnel2tunnel/
├── Cargo.toml                       # workspace (resolver = "2")
├── crates/
│   ├── tunnel2tunnel-core/          # DB models, queries, shared types, ip_whitelist
│   ├── tunnel2tunnel-web/           # Axum HTTP server + all API routes
│   ├── tunnel2tunnel-ssh/           # russh SSH server + tunnel router
│   ├── t2t/                         # binary entry point (joins all three)
│   └── t2t-feasibility/             # throw-away Phase 0 russh PoC
├── migrations/                      # SQLx migrations (applied in numeric order)
├── frontend/                        # Vue 3 SPA (Vite + TypeScript + SCSS)
│   └── src/
│       ├── api/         admin.ts  auth.ts  entities.ts  friends.ts
│       ├── components/  AppShell  EntityName  PubkeyInput  SshCommandDisplay
│       ├── pages/       Login  Dashboard  Entities  EntityDetail  Friends
│       │                AdminUsers  Settings  PurgeKeys  PurgeAccess
│       ├── stores/      auth  entities  friends   (Pinia)
│       ├── labels.ts    # central human-readable labels for all enum values
│       └── crypto.ts    # in-browser Ed25519 key-pair generation (Web Crypto)
├── contrib/fail2ban/filter.d/tunnel2tunnel.conf
├── Dockerfile
└── docker-compose.yml
```

---

## Running locally

```bash
# Start PostgreSQL (Podman — Docker Desktop is not used on this machine)
podman run -d --name t2t-pg \
  -e POSTGRES_USER=t2t -e POSTGRES_PASSWORD=t2t_secret -e POSTGRES_DB=tunnel2tunnel \
  -p 5432:5432 docker.io/postgres:18-alpine

# Build frontend
cd frontend && npm install && npm run build && cd ..

# Run backend (migrations applied on startup, admin bootstrapped once)
DATABASE_URL=postgres://t2t:t2t_secret@localhost/tunnel2tunnel \
ADMIN_USERNAME=admin ADMIN_PASSWORD=changeme \
HTTP_PORT=3000 SSH_PORT=2222 STATIC_DIR=frontend/dist \
RUST_LOG=info \
cargo run -p t2t
```

Browse to `http://localhost:3000`. Login: `admin` / `changeme`.

---

## Key technical facts

### Rust / backend

| Concern | Detail |
|---|---|
| Axum version | **0.8** — routes use `{param}` syntax, NOT `:param` |
| Path extractors | `Path<Uuid>` or `Path<(Uuid, Uuid)>` tuples — unaffected by `{param}` change |
| russh version | **0.61** — `Auth::Reject` requires `{ proceed_with_methods: None, partial_success: false }` |
| russh fingerprint | `key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256)` |
| SQLx | **0.8** — `#[sqlx(flatten)]` for nested structs, `#[sqlx(rename = "type")]` for reserved words |
| Sessions | `tower-sessions = "0.14"` + `tower-sessions-sqlx-store = "0.15"` — versions must match |
| UUIDs | `Uuid::now_v7()` in Rust; `uuidv7()` as PostgreSQL DEFAULT (built-in in PG 18, no extension) |
| Timestamps | `time = "0.3"` with `serde-well-known`; `#[serde(with = "time::serde::rfc3339")]` for ISO 8601 |
| Password hashing | `argon2 = "0.5"` |
| IP whitelist | Custom evaluator in `tunnel2tunnel-core/src/ip_whitelist.rs`; CIDR (`ipnetwork`), glob (`glob`), regex (`regex`), `!` prefix = deny; top-to-bottom first-match |

### Timestamp model structs — critical distinction

```rust
// Timestamps — fields directly on ts:
pub struct Timestamps { pub created_at: OffsetDateTime, pub updated_at: OffsetDateTime }

// TimestampsSoftDelete — fields nested under ts.timestamps and ts.soft_delete:
pub struct TimestampsSoftDelete {
    #[sqlx(flatten)] pub timestamps: Timestamps,
    #[sqlx(flatten)] pub soft_delete: SoftDelete,  // deleted_at: Option<OffsetDateTime>
}
```

- Models using `Timestamps`: `EntityAccess`, `Friendship`, `FriendshipEntityGrant`  
  → access via `.ts.created_at` directly
- Models using `TimestampsSoftDelete`: `User`, `Entity`, `SshKey`  
  → access via `.ts.timestamps.created_at`

### PostgreSQL

- Version: **18** (use `docker.io/postgres:18-alpine` or `podman`)
- `uuidv7()` is a native built-in — no `uuid-ossp` extension needed
- All timestamps: `TIMESTAMPTZ`, server/DB in UTC
- `set_timestamps()` trigger defined in `migrations/000_helpers.sql` — applied to every table

### Frontend

| Concern | Detail |
|---|---|
| Vue | 3 Composition API, `<script setup lang="ts">`, strict TypeScript (no `any`) |
| Build | Vite 5; `moduleResolution: bundler` — no Node types; use `new URL('./src', import.meta.url).pathname` for aliases, NOT `import { resolve } from 'path'` |
| Pinia | stores in `src/stores/` |
| Router | Vue Router 4; `meta: { requiresAuth: true }` on protected routes |
| SCSS | scoped per component |
| Enum labels | Always use `src/labels.ts` — never render raw `subject_type`, `visibility_grant`, etc. |
| Key filename | `PubkeyInput` takes `v-model:filename`; default `t2t_{entity_name}`; persisted to `localStorage` keyed `t2t_key_filename_{entityId}` |

---

## Migrations

| File | Contents |
|---|---|
| `000_helpers.sql` | `set_timestamps()` trigger function |
| `001_users.sql` | `users`, `settings` tables |
| `002_entities_keys_ports.sql` | `entities`, `ssh_keys`, `entity_ports` |
| `003_access_friends.sql` | `entity_access`, `friendships`, `friendship_entity_grants` |
| `004_connection_logs.sql` | `connection_logs` |

---

## Environment variables

| Var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | required | PostgreSQL connection string |
| `HTTP_PORT` | `3000` | HTTP server port |
| `SSH_PORT` | `2222` | SSH server port |
| `ADMIN_USERNAME` / `ADMIN_PASSWORD` | — | Bootstrap admin on first run (idempotent) |
| `STATIC_DIR` | `frontend/dist` | Path to compiled Vue SPA; skipped if directory absent |
| `FAIL2BAN_LOG_PATH` | — | If set, write sshd-format auth events to this file |
| `SSH_HOST_KEY_PATH` | `data/ssh_host_key` | Path where the SSH host key is persisted; mount as a volume in Docker |
| `SSH_T2T_KEY_PASSWORD` | — | If set, the stored host key is AES-256-CTR encrypted with this passphrase |
| `RUST_LOG` | — | Tracing filter, e.g. `info` or `tunnel2tunnel_ssh=debug` |

---

## API routes summary

All routes prefixed `/api/`. Auth: server-side session cookie.

```
POST   /api/auth/login            POST /api/auth/logout    GET /api/auth/me

GET    /api/entities              POST /api/entities
GET    /api/entities/{id}         PUT  /api/entities/{id}  DELETE /api/entities/{id}
POST   /api/entities/{id}/keys    DELETE /api/entities/{id}/keys/{key_id}
GET    /api/entities/{id}/ports   POST /api/entities/{id}/ports
PUT    /api/entities/{id}/ports/{port_id}   DELETE /api/entities/{id}/ports/{port_id}
GET    /api/entities/{id}/access  POST /api/entities/{id}/access
DELETE /api/entities/{id}/access/{rule_id}
GET    /api/entities/{id}/logs

GET    /api/friends               POST /api/friends
PUT    /api/friends/{id}
GET    /api/friends/{id}/grants   POST /api/friends/{id}/grants
DELETE /api/friends/{id}/grants/{entity_id}

GET    /api/admin/users           POST /api/admin/users
PUT    /api/admin/users/{id}

PUT    /api/me/password
GET    /api/me/ssh-keys           POST /api/me/purge-keys
GET    /api/me/access-rules       POST /api/me/purge-access
```

---

## SSH server (russh)

- Auth flow: fingerprint lookup → key/entity expiry → IP whitelist → `entity_access` check → admit
- Server slots: `Arc<Mutex<HashMap<(Uuid, u32), Handle>>>` keyed by `(entity_id, proxy_port)`
- `tcpip_forward` → parks a server session in the slot map
- `channel_open_direct_tcpip` → resolves hostname (UUID or `entity_access.hostname`), checks access, splices channels
- fail2ban log format: `<Month> <day> <time> t2t sshd[0]: Failed/Accepted publickey for …`

---

## In-browser key generation (`src/crypto.ts`)

Uses `window.crypto.subtle.generateKey({ name: 'Ed25519' })` (Chrome 113+, Firefox 130+, Safari 17+).  
Produces a correct OpenSSH wire-format private key (`openssh-key-v1\0`, unencrypted, blocksize-8 padding) and an authorized_keys public key line. Ed25519 is not yet in TypeScript's `SubtleCrypto` types — cast through `unknown` when calling `generateKey`.

---

## Known gotchas

- **Axum 0.8 routes**: use `{param}` not `:param` — the server panics on startup otherwise.
- **russh `Auth::Reject`**: must include `partial_success: false` field (required in 0.61).
- **`Timestamps` vs `TimestampsSoftDelete`**: see struct table above — wrong nesting causes SQLx compile errors.
- **tower-sessions versions**: `0.14` + `0.15` pair is required; mismatching breaks the session store trait.
- **Docker Desktop not available**: use Podman (`/usr/bin/podman`). No daemon needed.
- **Frontend alias**: `moduleResolution: bundler` has no Node types — do not `import { resolve } from 'path'`.
