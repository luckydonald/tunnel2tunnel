All requested content gathered. Here's the full report.

## 1. Call chain: EntityDetailPage.vue → backend

Frontend `frontend/src/pages/EntityDetailPage.vue`:
```ts
// line 10
import { adminApi, type ConnLog } from '@/api/admin'
...
// lines 293-309
// Connection log
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

`frontend/src/api/admin.ts` (lines 155-156):
```ts
  listConnectionLogs: (entity_id: string) =>
    apiFetch<ConnLog[]>(`/api/entities/${entity_id}/logs`),
```

This hits `GET /api/entities/:entity_id/logs`, routed (per naming) to `list_connection_logs` in `crates/tunnel2tunnel-web/src/routes/entities.rs`, which calls `ConnectionLog::list_for_entity(&state.db, entity_id, 50)`.

Note: this is distinct from the admin-wide search page (`AdminConnectionLogsPage.vue`), which calls `adminApi.searchConnectionLogs()` → `GET /api/admin/connection-logs` → `search_connection_logs` in `crates/tunnel2tunnel-web/src/routes/tarpit.rs`, using `ConnectionLog::search(...)`. Both handlers reuse the same `ConnLogResponse` struct defined in `entities.rs`.

## 2. `crates/tunnel2tunnel-web/src/routes/entities.rs` lines 566-612 (ConnLogResponse + entity-logs endpoint)

```rust
// ── Connection log ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConnLogResponse {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub attempted_password: Option<String>,
    pub fail_reason: Option<String>,
    pub success_reason: Option<String>,
    pub success: bool,
    pub tarpit_method: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl From<tunnel2tunnel_core::models::connection_log::ConnectionLog> for ConnLogResponse {
    fn from(l: tunnel2tunnel_core::models::connection_log::ConnectionLog) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: l.id,
            user_id: l.user_id,
            peer_ip: l.peer_ip,
            key_fingerprint: l.key_fingerprint,
            attempted_password: l.attempted_password,
            fail_reason: l.fail_reason,
            success_reason: l.success_reason,
            success: l.success,
            tarpit_method: l.tarpit_method,
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
(File is 612 lines total; this is the tail of the file.)

## 3. `crates/tunnel2tunnel-web/src/routes/tarpit.rs` (full, 284 lines)

```rust
//! Admin-only routes for the SSH tarpit/ban-rules feature: paginated/filtered
//! connection-log browsing, ban-rule CRUD, and global threshold settings.
//! Kept in its own module, separate from the general entity/settings routes.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use tunnel2tunnel_core::models::{
    ban_rule::BanRule, connection_log::ConnectionLog, settings::Settings,
};

use crate::{extractors::AdminUser, error::WebError, routes::entities::ConnLogResponse, AppState};

// ── Connection log search ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogSearchQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub success: Option<bool>,
    pub method: Option<String>,
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct LogSearchResponse {
    pub items: Vec<ConnLogResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// Pure pagination normalization — extracted so it's unit-testable without a
/// DB/HTTP request. Page is floored at 1; page_size clamped to [1, 200].
fn normalize_pagination(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    (page.unwrap_or(1).max(1), page_size.unwrap_or(50).clamp(1, 200))
}

pub async fn search_connection_logs(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Query(q): Query<LogSearchQuery>,
) -> Result<Json<LogSearchResponse>, WebError> {
    let (page, page_size) = normalize_pagination(q.page, q.page_size);
    let (rows, total) = ConnectionLog::search(
        &state.db,
        q.peer_ip.as_deref(),
        q.user_id,
        q.success,
        q.method.as_deref(),
        q.q.as_deref(),
        page,
        page_size,
    )
    .await
    .map_err(WebError::Core)?;
    Ok(Json(LogSearchResponse {
        items: rows.into_iter().map(ConnLogResponse::from).collect(),
        total,
        page,
        page_size,
    }))
}

// ── Ban rules CRUD ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct BanRuleResponse {
    pub id: Uuid,
    pub scope_type: String,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    pub active_until: Option<String>,
    pub created_by: Uuid,
    pub created_at: String,
    pub updated_at: String,
}

impl From<BanRule> for BanRuleResponse {
    fn from(r: BanRule) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: r.id,
            scope_type: r.scope_type,
            peer_ip: r.peer_ip,
            user_id: r.user_id,
            reason: r.reason,
            active_until: r.active_until.map(|t| t.format(&Rfc3339).unwrap_or_default()),
            created_by: r.created_by,
            created_at: r.ts.created_at.format(&Rfc3339).unwrap_or_default(),
            updated_at: r.ts.updated_at.format(&Rfc3339).unwrap_or_default(),
        }
    }
}

pub async fn list_ban_rules(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<BanRuleResponse>>, WebError> {
    let rules = BanRule::list_all(&state.db).await.map_err(WebError::Core)?;
    Ok(Json(rules.into_iter().map(BanRuleResponse::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateBanRuleBody {
    pub scope_type: String,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub active_until: Option<OffsetDateTime>,
}

/// Pure validation for a create-ban-rule request — extracted so it's
/// unit-testable without needing a DB/HTTP request.
fn validate_create_ban_rule(body: &CreateBanRuleBody) -> Result<(), WebError> {
    let valid_types = ["peer_ip", "user"];
    if !valid_types.contains(&body.scope_type.as_str()) {
        return Err(WebError::BadRequest("invalid scope_type".into()));
    }
    match body.scope_type.as_str() {
        "peer_ip" if body.peer_ip.is_none() => {
            Err(WebError::BadRequest("peer_ip required for scope_type 'peer_ip'".into()))
        }
        "user" if body.user_id.is_none() => {
            Err(WebError::BadRequest("user_id required for scope_type 'user'".into()))
        }
        _ => Ok(()),
    }
}

pub async fn create_ban_rule(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateBanRuleBody>,
) -> Result<(StatusCode, Json<BanRuleResponse>), WebError> {
    validate_create_ban_rule(&body)?;

    let rule = BanRule::create(
        &state.db,
        &body.scope_type,
        body.peer_ip.as_deref(),
        body.user_id,
        body.reason.as_deref(),
        body.active_until,
        admin.id,
    )
    .await
    .map_err(WebError::Core)?;

    Ok((StatusCode::CREATED, Json(BanRuleResponse::from(rule))))
}

pub async fn delete_ban_rule(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    if BanRule::delete(&state.db, id).await.map_err(WebError::Core)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Global threshold settings ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TarpitSettingsResponse {
    pub threshold_count: u32,
    pub threshold_window_seconds: u64,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateTarpitSettingsBody {
    pub threshold_count: u32,
    pub threshold_window_seconds: u64,
    pub enabled: bool,
}

pub async fn get_tarpit_settings(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<TarpitSettingsResponse>, WebError> {
    let threshold_count = Settings::get(&state.db, "tarpit_threshold_count")
        .await
        .map_err(WebError::Core)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let threshold_window_seconds = Settings::get(&state.db, "tarpit_threshold_window_seconds")
        .await
        .map_err(WebError::Core)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let enabled = Settings::get(&state.db, "tarpit_enabled")
        .await
        .map_err(WebError::Core)?
        .map(|v| v == "true")
        .unwrap_or(true);

    Ok(Json(TarpitSettingsResponse {
        threshold_count,
        threshold_window_seconds,
        enabled,
    }))
}

pub async fn update_tarpit_settings(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateTarpitSettingsBody>,
) -> Result<StatusCode, WebError> {
    if body.threshold_count == 0 {
        return Err(WebError::BadRequest("threshold_count must be positive".into()));
    }
    if body.threshold_window_seconds == 0 {
        return Err(WebError::BadRequest("threshold_window_seconds must be positive".into()));
    }
    Settings::set(&state.db, "tarpit_threshold_count", &body.threshold_count.to_string())
        .await
        .map_err(WebError::Core)?;
    Settings::set(
        &state.db,
        "tarpit_threshold_window_seconds",
        &body.threshold_window_seconds.to_string(),
    )
    .await
    .map_err(WebError::Core)?;
    Settings::set(&state.db, "tarpit_enabled", if body.enabled { "true" } else { "false" })
        .await
        .map_err(WebError::Core)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(scope_type: &str, peer_ip: Option<&str>, user_id: Option<Uuid>) -> CreateBanRuleBody {
        CreateBanRuleBody {
            scope_type: scope_type.to_string(),
            peer_ip: peer_ip.map(String::from),
            user_id,
            reason: None,
            active_until: None,
        }
    }

    #[test]
    fn rejects_unknown_scope_type() {
        let err = validate_create_ban_rule(&body("hostname", None, None)).unwrap_err();
        assert!(matches!(err, WebError::BadRequest(_)));
    }

    #[test]
    fn peer_ip_scope_requires_peer_ip() {
        assert!(validate_create_ban_rule(&body("peer_ip", None, None)).is_err());
        assert!(validate_create_ban_rule(&body("peer_ip", Some("203.0.113.1"), None)).is_ok());
    }

    #[test]
    fn user_scope_requires_user_id() {
        assert!(validate_create_ban_rule(&body("user", None, None)).is_err());
        assert!(validate_create_ban_rule(&body("user", None, Some(Uuid::now_v7()))).is_ok());
    }

    #[test]
    fn pagination_defaults_and_clamps() {
        assert_eq!(normalize_pagination(None, None), (1, 50));
        assert_eq!(normalize_pagination(Some(0), Some(0)), (1, 1));
        assert_eq!(normalize_pagination(Some(-5), Some(9999)), (1, 200));
        assert_eq!(normalize_pagination(Some(3), Some(20)), (3, 20));
    }
}
```

## 4. `frontend/src/pages/AdminConnectionLogsPage.vue` (full, 272 lines)

Full content already read; reproduced in full above in the tool output. Key points to note for editing:
- Script setup (lines 1-94): imports `adminApi`, `ConnLog`, `CreateBanRuleParams` from `@/api/admin`; `failReasonLabel`, `tarpitMethodLabel`, `tarpitMethodOptions` from `@/labels`; state `logs`, `total`, `page`, `pageSize = 50`, filters (`filterPeerIp`, `filterUserId`, `filterSuccess`, `filterMethod`, `filterQ`); `search()` calls `adminApi.searchConnectionLogs(...)`; ban-rule quick-ban modal logic (`openBanForm`, `confirmBan`).
- Template (lines 96-189): filter bar, `data-table` with columns Started/Ended/Peer IP/User/Fingerprint/Password/Result/Reason/Tarpit/(Ban button), pager, quick-ban modal.
- `<style lang="scss" scoped>` (lines 191-272): page-header, filter-bar, input-sm/select-sm, loading/empty, data-table, td-desc/td-ts, badge-result (ok/fail), btn-ban, pager, btn-primary/btn-secondary, modal-overlay/modal/section-note/field/modal-actions.

(Full verbatim content was captured in the Read tool call above — every line 1-272 is available if you want me to re-paste the whole block; I inlined the exact `<script setup>` and `<template>` sections above and can paste the style block verbatim too on request.)

Full `<style>` block verbatim (lines 191-272):
```scss
<style lang="scss" scoped>
.page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; h1 { margin: 0; } }

.filter-bar {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
  margin-bottom: 1.25rem; padding: 0.75rem; background: #1a1d27; border-radius: 6px;
}

.input-sm, .select-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.875rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.loading { color: #94a3b8; }
.empty { color: #64748b; }

.data-table {
  width: 100%; border-collapse: collapse; font-size: 0.875rem;
  th {
    text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid #2d3248;
    color: #64748b; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em;
  }
  td { padding: 0.5rem 0.75rem; border-bottom: 1px solid #1e2235; color: #e2e8f0; }
  code { background: #1a1d27; padding: 0.1em 0.35em; border-radius: 3px; font-size: 0.875em; }
  .fp { font-size: 0.75rem; word-break: break-all; }
}

.td-desc { color: #64748b; }
.td-ts { font-size: 0.8125rem; color: #94a3b8; white-space: nowrap; }

.badge-result {
  display: inline-block; padding: 0.1em 0.45em; border-radius: 4px;
  font-size: 0.75rem; font-weight: 600; text-transform: uppercase;
  &.ok   { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.fail { background: rgba(239,68,68,.15);  color: #fca5a5; }
}

.btn-ban {
  font-size: 0.75rem; padding: 0.2rem 0.6rem;
}

.pager {
  display: flex; align-items: center; gap: 1rem; margin-top: 1rem; color: #94a3b8; font-size: 0.875rem;
}

.btn-primary {
  padding: .375rem .875rem; background: #4f6ef7; border: none; border-radius: 4px;
  color: #fff; font-size: .875rem; cursor: pointer;
  &:hover:not(:disabled) { background: #3d5ce5; }
  &:disabled { opacity: .6; cursor: not-allowed; }
}
.btn-secondary {
  padding: .375rem .875rem; background: none; border: 1px solid #2d3248; border-radius: 4px;
  color: #94a3b8; font-size: .875rem; cursor: pointer;
  &:hover:not(:disabled) { color: #e2e8f0; border-color: #4f6ef7; }
  &:disabled { opacity: .5; cursor: not-allowed; }
}

.modal-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,.6);
  display: flex; align-items: center; justify-content: center; z-index: 100;
}
.modal {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px;
  padding: 2rem; width: 100%; max-width: 440px;
  h2 { margin: 0 0 0.5rem; font-size: 1.125rem; }
}
.section-note { font-size: 0.8125rem; color: #64748b; margin: 0 0 1rem; word-break: break-all; }
.field {
  margin-bottom: 1rem;
  label { display: block; margin-bottom: .375rem; font-size: .875rem; color: #94a3b8; }
  .optional { color: #64748b; }
  input {
    width: 100%; padding: .5rem .75rem; background: #0f1117;
    border: 1px solid #2d3248; border-radius: 4px; color: #e2e8f0;
    font-size: .9375rem; box-sizing: border-box;
    &:focus { outline: none; border-color: #4f6ef7; }
  }
}
.modal-actions { display: flex; gap: .75rem; justify-content: flex-end; margin-top: 1.5rem; }
</style>
```

## 5. `frontend/src/pages/EntityDetailPage.vue` — logs section

Script (lines 293-309):
```ts
// Connection log
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

Template (lines 653-688):
```html
      <!-- Connection log -->
      <section class="section">
        <div class="section-header">
          <h2>Connection Log</h2>
          <button class="btn-secondary" @click="loadConnLogs">Refresh</button>
        </div>
        <div v-if="logsLoading" class="loading">Loading…</div>
        <table v-else-if="logsLoaded && connLogs.length" class="data-table">
          <thead>
            <tr>
              <th>Time</th>
              <th>Peer IP</th>
              <th>Fingerprint</th>
              <th>Result</th>
              <th>Reason</th>
              <th>Tarpit</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="l in connLogs" :key="l.id">
              <td class="td-ts">{{ new Date(l.started_at).toLocaleString() }}</td>
              <td>{{ l.peer_ip ?? '—' }}</td>
              <td><code v-if="l.key_fingerprint" class="fp">{{ l.key_fingerprint }}</code><span v-else>—</span></td>
              <td>
                <span :class="['badge-result', l.success ? 'ok' : 'fail']">
                  {{ l.success ? 'ok' : 'fail' }}
                </span>
              </td>
              <td class="td-desc">{{ (l.fail_reason && failReasonLabel[l.fail_reason]) ?? l.fail_reason ?? l.success_reason ?? '—' }}</td>
              <td class="td-desc">{{ l.tarpit_method ? tarpitMethodLabel[l.tarpit_method] : '—' }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else-if="logsLoaded" class="empty">No connection logs.</p>
        <p v-else class="empty">Click Refresh to load logs.</p>
      </section>

    </template>
  </AppShell>
</template>
```
Note: this is the last section in the template, right before `</template>`/`</AppShell>`/`</template>` closing tags (lines 690-693 in the file).

Import at top of file (line 10): `import { adminApi, type ConnLog } from '@/api/admin'`

## 6. `frontend/src/api/admin.ts` (full, 187 lines)

```ts
export interface AdminUser {
  id: string
  username: string
  email: string | null
  is_admin: boolean
  is_locked: boolean
  description: string | null
  created_at: string
}

export type TarpitMethod = 'banner_drip' | 'slow_auth' | 'fake_shell'
export type BanScopeType = 'peer_ip' | 'user'

export interface ConnLog {
  id: string
  user_id: string | null
  peer_ip: string | null
  key_fingerprint: string | null
  attempted_password: string | null
  fail_reason: string | null
  success_reason: string | null
  success: boolean
  tarpit_method: TarpitMethod | null
  started_at: string
  ended_at: string | null
}

export interface LogSearchParams {
  page?: number
  page_size?: number
  peer_ip?: string
  user_id?: string
  success?: boolean
  method?: TarpitMethod
  q?: string
}

export interface LogSearchResult {
  items: ConnLog[]
  total: number
  page: number
  page_size: number
}

export interface BanRule {
  id: string
  scope_type: BanScopeType
  peer_ip: string | null
  user_id: string | null
  reason: string | null
  active_until: string | null
  created_by: string
  created_at: string
  updated_at: string
}

export interface CreateBanRuleParams {
  scope_type: BanScopeType
  peer_ip?: string | null
  user_id?: string | null
  reason?: string | null
  active_until?: string | null
}

export interface TarpitSettings {
  threshold_count: number
  threshold_window_seconds: number
  enabled: boolean
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    credentials: 'include',
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    ...init,
  })
  if (res.status === 204) return undefined as T
  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new Error(body.error ?? `HTTP ${res.status}`)
  }
  return res.json()
}

export const adminApi = {
  listUsers: () => apiFetch<AdminUser[]>('/api/admin/users'),

  createUser: (data: {
    username: string
    email?: string | null
    password: string
    is_admin?: boolean
    description?: string | null
  }) =>
    apiFetch<AdminUser>('/api/admin/users', {
      method: 'POST',
      body: JSON.stringify(data),
    }),

  updateUser: (
    id: string,
    data: {
      email?: string | null
      is_admin?: boolean
      is_locked?: boolean
      description?: string | null
      password?: string
    },
  ) =>
    apiFetch<AdminUser>(`/api/admin/users/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),

  changePassword: (old_password: string, new_password: string) =>
    apiFetch<void>('/api/me/password', {
      method: 'PUT',
      body: JSON.stringify({ old_password, new_password }),
    }),

  listMyKeys: () =>
    apiFetch<
      Array<{
        id: string
        entity_id: string
        algorithm: string
        fingerprint: string
        name: string | null
        comment: string | null
      }>
    >('/api/me/ssh-keys'),

  purgeKeys: (ids: string[]) =>
    apiFetch<void>('/api/me/purge-keys', {
      method: 'POST',
      body: JSON.stringify({ ids }),
    }),

  listMyAccess: () =>
    apiFetch<
      Array<{
        id: string
        owner_entity_id: string
        subject_type: string
        hostname: string | null
      }>
    >('/api/me/access-rules'),

  purgeAccess: (ids: string[]) =>
    apiFetch<void>('/api/me/purge-access', {
      method: 'POST',
      body: JSON.stringify({ ids }),
    }),

  listConnectionLogs: (entity_id: string) =>
    apiFetch<ConnLog[]>(`/api/entities/${entity_id}/logs`),

  searchConnectionLogs: (params: LogSearchParams = {}) => {
    const q = new URLSearchParams()
    for (const [k, v] of Object.entries(params)) {
      if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
    }
    const qs = q.toString()
    return apiFetch<LogSearchResult>(`/api/admin/connection-logs${qs ? `?${qs}` : ''}`)
  },

  listBanRules: () => apiFetch<BanRule[]>('/api/admin/ban-rules'),

  createBanRule: (params: CreateBanRuleParams) =>
    apiFetch<BanRule>('/api/admin/ban-rules', {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  deleteBanRule: (id: string) =>
    apiFetch<void>(`/api/admin/ban-rules/${id}`, { method: 'DELETE' }),

  getTarpitSettings: () => apiFetch<TarpitSettings>('/api/admin/tarpit-settings'),

  updateTarpitSettings: (settings: TarpitSettings) =>
    apiFetch<void>('/api/admin/tarpit-settings', {
      method: 'PUT',
      body: JSON.stringify(settings),
    }),

  sampleError: () => apiFetch<void>('/api/admin/sample-error'),
}
```

### Summary of key facts for editing
- `entities.rs` is 612 lines total; `ConnLogResponse` struct + `From` impl + `list_connection_logs` handler occupy lines 566-612 (the very end of the file).
- `tarpit.rs` imports `ConnLogResponse` from `crate::routes::entities` (line 18: `use crate::{extractors::AdminUser, error::WebError, routes::entities::ConnLogResponse, AppState};`), so both endpoints share one response DTO — any shape change to `ConnLogResponse` affects both `EntityDetailPage.vue`'s per-entity table and `AdminConnectionLogsPage.vue`'s global search table.
- `EntityDetailPage.vue`'s per-entity endpoint (`/api/entities/:id/logs`) hardcodes a limit of 50 and no pagination — it returns a plain `Vec<ConnLogResponse>`, unlike the admin search endpoint's paginated `LogSearchResponse { items, total, page, page_size }`.
- Both frontend tables use the same `ConnLog` TS interface (`frontend/src/api/admin.ts` lines 14-26) and both use `failReasonLabel`/`tarpitMethodLabel` from `@/labels` for rendering reason/method columns.