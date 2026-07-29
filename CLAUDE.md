# tunnel2tunnel

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
| Sentry | `sentry = "0.48.3"`; init lives in `crates/t2t/src/sentry.rs`, called from `main()` before the tokio runtime starts (no `#[tokio::main]`); `sentry-tower`'s `NewSentryLayer`/`SentryHttpLayer` wrap the axum router in `tunnel2tunnel-web/src/lib.rs` for request-correlated error capture. No `dist` field on `ClientOptions` in this version — build time is set as a scope tag instead. |

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
| `SENTRY_DSN` | — | Backend Sentry/Bugsink DSN. Empty/unset = reporting disabled. |
| `SENTRY_ENVIRONMENT` | — | e.g. `production`, `development` |
| `SENTRY_RELEASE` | git commit (`SOURCE_COMMIT` → `git rev-parse HEAD`) | Explicit override for the release tag |
| `SENTRY_TRACES_SAMPLE_RATE` | `0` | `0`–`1`; omit or `0` to disable tracing spans |
| `VITE_SENTRY_DSN` | — | Frontend DSN, baked in at **build time**. Separate Bugsink project from the backend for this deployment. |
| `VITE_SENTRY_ENVIRONMENT` / `VITE_SENTRY_RELEASE` / `VITE_SENTRY_TRACES_SAMPLE_RATE` | — | Frontend equivalents of the backend vars above |
| `SOURCE_COMMIT` / `GIT_BRANCH` / `BUILD_TIME` | — | Release/build metadata tags shared by frontend+backend. Auto-detected via `git` when `.git` is present (local dev); must be passed explicitly as Docker **build args** for the frontend (Dockerfile's `node-builder` stage has no `.git`) and as container env for the backend. |
| `BUILD_BUGSINK_URL` / `BUILD_BUGSINK_AUTH_TOKEN` / `BUILD_BUGSINK_PROJECT_SLUG` | — | Build-time-only secrets (deliberately not `VITE_`-prefixed) for uploading frontend sourcemaps via `@sentry/vite-plugin`; build succeeds without them, just skips the upload |

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

## Skills

- activate [ @ai/skills/commit-with-lplp-style/SKILL.md ](ai/skills/commit-with-lplp-style/SKILL.md) and [ @ai/skills/code-style/references/vue.md ](ai/skills/code-style/references/vue.md) automatically.

---

## Known gotchas

- **Axum 0.8 routes**: use `{param}` not `:param` — the server panics on startup otherwise.
- **russh `Auth::Reject`**: must include `partial_success: false` field (required in 0.61).
- **russh `Handle` confirmation calls** (e.g. `channel_open_session()`): must be `tokio::spawn`ed, never `.await`ed inline from a `Handler` callback (e.g. `auth_succeeded`) — the confirmation round-trips through that same connection's single-task event loop, so awaiting it synchronously inside the callback deadlocks permanently.
- **`Timestamps` vs `TimestampsSoftDelete`**: see struct table above — wrong nesting causes SQLx compile errors.
- **tower-sessions versions**: `0.14` + `0.15` pair is required; mismatching breaks the session store trait.
- **Docker Desktop not available**: use Podman (`/usr/bin/podman`). No daemon needed.
- **Frontend alias**: no Node types in browser-shipped `src/**` code — do not `import { resolve } from 'path'` there. `vite.config.ts` itself *does* run in Node at build time and legitimately uses `node:child_process`/`process.env` (for git commit/build-time metadata) — `@types/node` is a devDependency for that reason, but only `vite.config.ts` should rely on it.
- **Sentry init must precede the tokio runtime**: `crates/t2t/src/main.rs` can't use `#[tokio::main]` — it builds the runtime by hand in `main()` after `sentry::init_sentry()` so the panic hook covers tasks spawned during runtime startup too.
- **Panic-returning axum handlers**: a handler body that only `panic!()`s can't return `impl IntoResponse` (E0560 "never type fallback" — opaque `impl Trait` return types don't coerce from `!`). Give it a concrete return type instead, e.g. `axum::http::StatusCode` (see `routes/diagnostics.rs::sample_error`).
- **`sentry` 0.48.3 `ClientOptions` has no `dist` field** (removed from older SDK versions) — tag build time via `sentry::configure_scope` instead.
- **Building on a <200MB RAM VPS**: `cargo build --release` and `npm run build` both need far more than 200MB peak RSS — a swapfile on the host is mandatory, the build changes in the Dockerfile only reduce how much of it gets used. Don't add `sentry`'s `rustls` feature to save build memory — it looks lighter (drops `native-tls`/`openssl-sys`) but reqwest's `rustls` feature hard-codes the `aws-lc-rs` crypto provider, which compiles a vendored C library via cmake; that's heavier than `native-tls`, which just links against Debian's already-built `libssl` via `pkg-config` (no compilation). Keep `native-tls`.
