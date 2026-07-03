export interface AdminUser {
  id: string
  username: string
  email: string | null
  is_admin: boolean
  is_locked: boolean
  description: string | null
  created_at: string
}

export interface ConnLog {
  id: string
  peer_ip: string | null
  key_fingerprint: string | null
  login_succeeded: boolean
  failure_reason: string | null
  started_at: string
  ended_at: string | null
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

  sampleError: () => apiFetch<void>('/api/admin/sample-error'),
}
