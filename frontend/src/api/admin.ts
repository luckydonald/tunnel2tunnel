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
  attempted_username: string | null
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
  enabled: boolean
}

export interface TarpitThreshold {
  id: string
  fail_count: number
  window_seconds: number
  enabled: boolean
  created_at: string
  updated_at: string
}

export interface TarpitThresholdParams {
  fail_count: number
  window_seconds: number
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

  listTarpitThresholds: () => apiFetch<TarpitThreshold[]>('/api/admin/tarpit-thresholds'),

  createTarpitThreshold: (params: TarpitThresholdParams) =>
    apiFetch<TarpitThreshold>('/api/admin/tarpit-thresholds', {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  updateTarpitThreshold: (id: string, params: TarpitThresholdParams) =>
    apiFetch<TarpitThreshold>(`/api/admin/tarpit-thresholds/${id}`, {
      method: 'PUT',
      body: JSON.stringify(params),
    }),

  deleteTarpitThreshold: (id: string) =>
    apiFetch<void>(`/api/admin/tarpit-thresholds/${id}`, { method: 'DELETE' }),

  sampleError: () => apiFetch<void>('/api/admin/sample-error'),
}
