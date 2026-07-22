I now have everything needed. Here is the full end-to-end report.

## 1. `migrations/004_connection_logs.sql` (full contents)

```sql
CREATE TABLE connection_logs (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    entity_id       UUID REFERENCES entities(id) ON DELETE SET NULL,
    peer_ip         TEXT,
    key_fingerprint TEXT,
    login_succeeded BOOL NOT NULL,
    failure_reason  TEXT,
    ssh_flags       TEXT,
    ports_requested TEXT,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at        TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON connection_logs
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

CREATE INDEX connection_logs_entity_id_idx ON connection_logs(entity_id);
CREATE INDEX connection_logs_started_at_idx ON connection_logs(started_at DESC);
```

Note: `failure_reason` is free‑text `TEXT`, no `CHECK` constraint — it's an informal, ad-hoc "enum" (see part 6). `ports_requested` column exists in the schema but is never populated/read anywhere in the Rust code (grep shows no reference outside this migration).

Latest migrations in repo: `000_helpers.sql` … `007_entity_port_host.sql`. No tarpit/ban table exists yet.

## 2. Core model & queries — `crates/tunnel2tunnel-core/src/models/connection_log.rs`

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectionLog {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub login_succeeded: bool,
    pub failure_reason: Option<String>,
    pub ssh_flags: Option<String>,
    pub ports_requested: Option<String>,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    pub ts: Timestamps,
}
```

- `create(...)` (lines 26-55): inserts a row with `RETURNING *`, `id = Uuid::now_v7()`.
- `set_ended(pool, id)` (57-64): `UPDATE connection_logs SET ended_at = NOW() WHERE id = $1`.
- `list_for_entity(pool, entity_id, limit)` (66-80):
```rust
"SELECT * FROM connection_logs \
 WHERE entity_id = $1 ORDER BY started_at DESC LIMIT $2"
```
**There is no pagination (offset/cursor), no filter, and no search query function at all** — only a single `entity_id` scope + fixed `LIMIT`. This is the biggest gap vs. what you want to build.

Writers of this log (`crates/tunnel2tunnel-ssh/src/lib.rs`):
- `log_auth_failure` (lines 164-197) calls `ConnectionLog::create(pool, None, Some(peer_ip), fingerprint, false, Some(reason), None, now)` with free-text `reason` strings, e.g. lines 255, 315, 330, 338, 348, 363, 377: `"password auth not supported"`, `"unknown key"`, `"key expired"`, `"entity not found"`, `"IP blocked by whitelist"`, etc. This is exactly where tarpit/ban decisions would also need to hook in.
- `log_auth_success` (199-225) calls `ConnectionLog::create(pool, Some(entity.id), Some(peer_ip), Some(fingerprint), true, None, None, now)`, stores `log_id` on the handler, later calls `ConnectionLog::set_ended(&pool, log_id)` at line 820 when the session ends.

## 3. Web API route — `crates/tunnel2tunnel-web/src/routes/entities.rs`

```rust
// ── Connection log ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConnLogResponse {
    pub id: Uuid,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub login_succeeded: bool,
    pub failure_reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl From<tunnel2tunnel_core::models::connection_log::ConnectionLog> for ConnLogResponse {
    fn from(l: tunnel2tunnel_core::models::connection_log::ConnectionLog) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: l.id,
            peer_ip: l.peer_ip,
            key_fingerprint: l.key_fingerprint,
            login_succeeded: l.login_succeeded,
            failure_reason: l.failure_reason,
            started_at: l.started_at.format(&Rfc3339).unwrap_or_default(),
            ended_at: l.ended_at.map(|t| t.format(&Rfc3339).unwrap_or_default()),
        }
    }
}

pub async fn list_connection_logs(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<ConnLogResponse>>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let logs = ConnectionLog::list_for_entity(&state.db, entity_id, 50)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(logs.into_iter().map(ConnLogResponse::from).collect()))
}
```
(lines 535-573). Route registration in `crates/tunnel2tunnel-web/src/lib.rs:101-102`:
```rust
.route("/api/entities/{id}/logs",
    get(routes::entities::list_connection_logs))
```
No query-param parsing exists (no `Query<...>` extractor) — no pagination, filter, or search params in the current API. Response shape is a flat `Vec<ConnLogResponse>` JSON array (no envelope/total-count for pagination).

## 4. Frontend rendering — `frontend/src/pages/EntityDetailPage.vue` + `frontend/src/api/admin.ts`

`admin.ts` (lines 11-19, 105-106):
```ts
export interface ConnLog {
  id: string
  peer_ip: string | null
  key_fingerprint: string | null
  login_succeeded: boolean
  failure_reason: string | null
  started_at: string
  ended_at: string | null
}
...
listConnectionLogs: (entity_id: string) =>
  apiFetch<ConnLog[]>(`/api/entities/${entity_id}/logs`),
```

`EntityDetailPage.vue` (lines 293-309, 645-678) — the whole feature is a "Connection Log" `<section>` inside the entity detail page, lazy-loaded on demand:
```vue
const connLogs = ref<ConnLog[]>([])
const logsLoaded = ref(false)
const logsLoading = ref(false)

async function loadConnLogs(): Promise<void> {
  if (logsLoaded.value) return
  logsLoading.value = true
  try {
    connLogs.value = await adminApi.listConnectionLogs(entityId)
    logsLoaded.value = true
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load logs')
  } finally {
    logsLoading.value = false
  }
}
```
```vue
<!-- Connection log -->
<section class="section">
  <div class="section-header">
    <h2>Connection Log</h2>
    <button class="btn-secondary" @click="loadConnLogs">Refresh</button>
  </div>
  <div v-if="logsLoading" class="loading">Loading…</div>
  <table v-else-if="logsLoaded && connLogs.length" class="data-table">
    <thead>
      <tr><th>Time</th><th>Peer IP</th><th>Fingerprint</th><th>Result</th><th>Reason</th></tr>
    </thead>
    <tbody>
      <tr v-for="l in connLogs" :key="l.id">
        <td class="td-ts">{{ new Date(l.started_at).toLocaleString() }}</td>
        <td>{{ l.peer_ip ?? '—' }}</td>
        <td><code v-if="l.key_fingerprint" class="fp">{{ l.key_fingerprint }}</code><span v-else>—</span></td>
        <td>
          <span :class="['badge-result', l.login_succeeded ? 'ok' : 'fail']">
            {{ l.login_succeeded ? 'ok' : 'fail' }}
          </span>
        </td>
        <td class="td-desc">{{ l.failure_reason ?? '—' }}</td>
      </tr>
    </tbody>
  </table>
  <p v-else-if="logsLoaded" class="empty">No connection logs.</p>
  <p v-else class="empty">Click Refresh to load logs.</p>
</section>
```
**No pagination or filter/search UI exists** — just a "Refresh" button that fetches the fixed 50-row list once and caches (`logsLoaded` gate prevents refetch except via the button). You'll need to build pagination/filter/search from scratch for both API and UI; there's no precedent component for it in this codebase (no generic data-table/pagination component anywhere in `frontend/src/components`).

`labels.ts` — no connection-log-specific labels exist today (only `subjectTypeLabel`, `visibilityGrantLabel`, `friendshipStatusLabel`, `entityTypeLabel`, all built the same way: `Record<UnionType, string>` + a derived `Options` array via `Object.entries(...).map(...)`, see quote below in part 6).

## 5. Settings / admin-rule-list patterns

**Global key-value `settings` table** — `migrations/001_users.sql:18-25`:
```sql
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT INTO settings (key, value) VALUES
  ('signup_enabled',    'false'),
  ('fail2ban_log_path', '');
```
Core model `crates/tunnel2tunnel-core/src/models/settings.rs`:
```rust
pub struct Settings;
impl Settings {
    pub async fn get(pool: &PgPool, key: &str) -> Result<Option<String>, CoreError> { ... }
    pub async fn set(pool: &PgPool, key: &str, value: &str) -> Result<(), CoreError> { ... }
}
```
**Important finding: this `Settings` model has zero callers anywhere else in the codebase** (`grep -rln "Settings"` only matches its own file). `fail2ban_log_path` is actually read from an env var (`FAIL2BAN_LOG_PATH`) in `crates/t2t/src/main.rs:39`, not from this DB table, and `signup_enabled` isn't read anywhere. So the key-value `settings` table is effectively dead/unused scaffolding today — not a live pattern to imitate for admin-configurable rules.

**`SettingsPage.vue`** (`frontend/src/pages/SettingsPage.vue`) currently only has: Server Information (read-only), Change Password form, and links to `PurgeKeysPage`/`PurgeAccessPage`. There is no generic settings-editing UI, and no UI reads/writes the `settings` table at all.

**IP whitelist "rule list"** is actually per-entity free text, not a separate admin rules table:
- Column: `migrations/002_entities_keys_ports.sql:7` → `ip_whitelist TEXT` on `entities`.
- Evaluator: `crates/tunnel2tunnel-core/src/ip_whitelist.rs` — newline-separated rules (IP/CIDR/glob/regex, `!` prefix = deny, first match wins, evaluated top-to-bottom, empty = allow-all).
- Wired to entity CRUD in `crates/tunnel2tunnel-core/src/models/entity.rs` (create/update take `ip_whitelist: Option<&str>`) and `crates/tunnel2tunnel-web/src/routes/entities.rs` (CreateEntityBody/UpdateEntityBody/EntityResponse all carry `ip_whitelist`), and TS types in `frontend/src/api/entities.ts:6,63,70`.
- **However there is no UI to edit `ip_whitelist` anywhere in the frontend** — no textarea for it in `EntityDetailPage.vue` or elsewhere; it's plumbed through the API but not exposed in the Vue admin at present. So there's no existing free-text-rules editor UI to copy either.

**The actual reusable "rules list" CRUD pattern** is the **Access Rules** section in `EntityDetailPage.vue` (lines 171-277 script, 514-617 template), backed by `entity_access` table (`migrations/003_access_friends.sql:2-16`) and `crates/tunnel2tunnel-core/src/models/entity_access.rs`. This is the best template to copy for a new "tarpit/ban rules by peer_ip or by user" admin feature: a toggleable "Add rule" form (select for rule-type enum + conditional sub-fields + free-text field), a `data-table` listing existing rules with a delete (`×`) button per row, lazy-loaded via a `xLoaded` boolean guard. Quoted in full above — reuse this shape (add-form toggle, `blank...()` factory, `handleAdd...`/`handleDelete...`, `data-table` with delete button) for your tarpit-rule editor.

Also relevant: `PurgeAccessPage.vue` / `PurgeKeysPage.vero` for the "select multiple rows + bulk action" pattern (`/api/me/purge-access`, `/api/me/purge-keys`), if bulk-unban is needed.

## 6. Enum representation across DB ↔ Rust ↔ TS (example: `subject_type` / `visibility_grant`)

**DB layer** — plain `TEXT` + `CHECK` constraint (no native Postgres enum type used anywhere in this codebase):
```sql
-- migrations/003_access_friends.sql
subject_type TEXT NOT NULL CHECK(subject_type IN (
    'entity', 'all_mine', 'all_user_entities', 'public_lite')),
...
visibility_grant TEXT NOT NULL DEFAULT 'none'
    CHECK(visibility_grant IN ('none', 'clients', 'servers', 'all')),
```

**Rust core model** — just `String`, no Rust enum type at all (`crates/tunnel2tunnel-core/src/models/entity_access.rs:12`):
```rust
pub struct EntityAccess {
    ...
    pub subject_type: String,
    ...
}
```

**Rust web layer validation** — manual runtime check against a literal array, in the handler (`crates/tunnel2tunnel-web/src/routes/access.rs:76-95`):
```rust
#[derive(Deserialize)]
pub struct CreateAccessBody {
    pub subject_type: String,
    ...
}

pub async fn create_access(...) -> Result<..., WebError> {
    require_entity_owner(&state, entity_id, user.id).await?;
    let valid_types = ["entity", "all_mine", "all_user_entities", "public_lite"];
    if !valid_types.contains(&body.subject_type.as_str()) {
        return Err(WebError::BadRequest("invalid subject_type".into()));
    }
    ...
}
```

**TypeScript API layer** — string literal union type (`frontend/src/api/friends.ts:6,21`):
```ts
export interface Friendship {
  ...
  visibility_grant: 'none' | 'clients' | 'servers' | 'all'
}
export interface AccessRule {
  ...
  subject_type: 'entity' | 'all_mine' | 'all_user_entities' | 'public_lite'
}
```

**TypeScript display-label layer** — `frontend/src/labels.ts:1-23`:
```ts
export const subjectTypeLabel: Record<AccessRule['subject_type'], string> = {
  public_lite:        'Anyone (no account required)',
  all_mine:           'All my own entities',
  all_user_entities:  'All entities of a specific user',
  entity:             'One specific entity',
}

export const subjectTypeOptions = (
  Object.entries(subjectTypeLabel) as [AccessRule['subject_type'], string][]
).map(([value, label]) => ({ value, label }))

export const visibilityGrantLabel: Record<Friendship['visibility_grant'], string> = {
  none:    'Hidden (no visibility)',
  clients: 'Clients only',
  servers: 'Servers only',
  all:     'All entities',
}
export const visibilityGrantOptions = (
  Object.entries(visibilityGrantLabel) as [Friendship['visibility_grant'], string][]
).map(([value, label]) => ({ value, label }))
```
These `xxxOptions` arrays feed `<select>` dropdowns (e.g. `subjectTypeOptions` used at `EntityDetailPage.vue:527-531`), and the `xxxLabel` maps are used for read-only display (e.g. `EntityDetailPage.vue:590`: `{{ subjectTypeLabel[rule.subject_type] }}`).

**Recommended pattern for your new "tarpit method" and "fail reason"/"success reason" enums**: follow this exact four-layer stack — `TEXT` + `CHECK (... IN (...))` in the migration, plain `String` field in the Rust `sqlx::FromRow` struct (no Rust enum, validate with a `valid_types`-style array check in the web handler, matching `access.rs:92-95`), a TS string-literal union in the corresponding `api/*.ts` interface, and a `Record<Union, string>` + derived `Options` array in `labels.ts` for the admin UI dropdowns/badges. Note today's `failure_reason` in `connection_logs` breaks this pattern (free `TEXT`, no `CHECK`, ad-hoc strings from `tunnel2tunnel-ssh/src/lib.rs`) — for your new table you likely want to tighten this to a real constrained enum from the start, unlike the existing connection-log's loose `failure_reason`.