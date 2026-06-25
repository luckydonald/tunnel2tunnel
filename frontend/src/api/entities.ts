export interface Entity {
  id: string
  entity_type: string
  name: string | null
  description: string | null
  ip_whitelist: string | null
  valid_until: string | null
  created_at: string
  updated_at: string
  deleted_at: string | null
}

export interface SshKey {
  id: string
  entity_id: string
  algorithm: string
  key_data: string
  comment: string | null
  fingerprint: string
  name: string | null
  valid_until: string | null
  created_at: string
  updated_at: string
  deleted_at: string | null
}

export interface EntityPort {
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

export interface EntityDetail extends Entity {
  ssh_keys: SshKey[]
  ports: EntityPort[]
}

export interface CreateEntityParams {
  entity_type: 'server' | 'client'
  name?: string | null
  description?: string | null
  ip_whitelist?: string | null
  valid_until?: string | null
}

export interface UpdateEntityParams {
  name?: string | null
  description?: string | null
  ip_whitelist?: string | null
  valid_until?: string | null
}

export interface AddKeyParams {
  algorithm: string
  key_data: string
  comment?: string | null
  name?: string | null
  valid_until?: string | null
}

export interface CreatePortParams {
  enabled?: boolean
  local_port: number
  proxy_port: number
  name?: string | null
  description?: string | null
  sort_order?: number
}

export interface UpdatePortParams {
  enabled: boolean
  local_port: number
  proxy_port: number
  name?: string | null
  description?: string | null
  sort_order: number
}

async function apiFetch<T>(url: string, options?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    ...options,
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText })) as { error?: string }
    throw new Error(body.error ?? res.statusText)
  }
  if (res.status === 204) return undefined as unknown as T
  return res.json() as Promise<T>
}

export const entitiesApi = {
  list: (entityType?: 'server' | 'client') => {
    const q = entityType ? `?entity_type=${entityType}` : ''
    return apiFetch<Entity[]>(`/api/entities${q}`)
  },

  create: (params: CreateEntityParams) =>
    apiFetch<Entity>('/api/entities', {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  getEntity: (id: string) =>
    apiFetch<EntityDetail>(`/api/entities/${id}`),

  update: (id: string, params: UpdateEntityParams) =>
    apiFetch<Entity>(`/api/entities/${id}`, {
      method: 'PUT',
      body: JSON.stringify(params),
    }),

  deleteEntity: (id: string) =>
    apiFetch<void>(`/api/entities/${id}`, { method: 'DELETE' }),

  addKey: (entityId: string, params: AddKeyParams) =>
    apiFetch<SshKey>(`/api/entities/${entityId}/keys`, {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  deleteKey: (entityId: string, keyId: string) =>
    apiFetch<void>(`/api/entities/${entityId}/keys/${keyId}`, { method: 'DELETE' }),

  createPort: (entityId: string, params: CreatePortParams) =>
    apiFetch<EntityPort>(`/api/entities/${entityId}/ports`, {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  updatePort: (entityId: string, portId: string, params: UpdatePortParams) =>
    apiFetch<EntityPort>(`/api/entities/${entityId}/ports/${portId}`, {
      method: 'PUT',
      body: JSON.stringify(params),
    }),

  deletePort: (entityId: string, portId: string) =>
    apiFetch<void>(`/api/entities/${entityId}/ports/${portId}`, { method: 'DELETE' }),
}
