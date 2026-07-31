export interface Entity {
  id: string
  name: string | null
  description: string | null
  ip_whitelist: string | null
  valid_until: string | null
  created_at: string
  updated_at: string
  deleted_at: string | null
  online: boolean
  last_disconnected_at: string | null
  is_server: boolean
  is_client: boolean
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

export interface PortConfig {
  id: string
  entity_id: string
  enabled: boolean
  local_port: number
  proxy_port: number
  name: string
  description: string | null
  sort_order: number
  host: string
  created_at: string
  updated_at: string
}

export interface PortSubscription {
  id: string
  port_config_id: string
  subscriber_entity_id: string
  subscriber_local_port: number
  enabled: boolean
  created_at: string
  updated_at: string
}

export interface SubscribableService extends PortConfig {
  subscription: PortSubscription | null
}

export interface SubscribableOwner extends Entity {
  services: SubscribableService[]
}

export interface EntityDetail extends Entity {
  ssh_keys: SshKey[]
  ports: PortConfig[]
}

export interface CreateEntityParams {
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
  name: string
  description?: string | null
  sort_order?: number
  host?: string
}

export interface UpdatePortParams {
  enabled: boolean
  local_port: number
  proxy_port: number
  name: string
  description?: string | null
  sort_order: number
  host?: string
}

export interface CreateSubscriptionParams {
  port_config_id: string
  subscriber_local_port: number
  enabled?: boolean
}

export interface UpdateSubscriptionParams {
  subscriber_local_port?: number
  enabled?: boolean
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
  list: (role?: 'server' | 'client') => {
    const q = role ? `?role=${role}` : ''
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
    apiFetch<PortConfig>(`/api/entities/${entityId}/ports`, {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  updatePort: (entityId: string, portId: string, params: UpdatePortParams) =>
    apiFetch<PortConfig>(`/api/entities/${entityId}/ports/${portId}`, {
      method: 'PUT',
      body: JSON.stringify(params),
    }),

  deletePort: (entityId: string, portId: string) =>
    apiFetch<void>(`/api/entities/${entityId}/ports/${portId}`, { method: 'DELETE' }),

  getSubscribableServices: (entityId: string) =>
    apiFetch<SubscribableOwner[]>(`/api/entities/${entityId}/subscribable-services`),

  createSubscription: (entityId: string, params: CreateSubscriptionParams) =>
    apiFetch<PortSubscription>(`/api/entities/${entityId}/subscriptions`, {
      method: 'POST',
      body: JSON.stringify(params),
    }),

  updateSubscription: (entityId: string, subscriptionId: string, params: UpdateSubscriptionParams) =>
    apiFetch<PortSubscription>(`/api/entities/${entityId}/subscriptions/${subscriptionId}`, {
      method: 'PUT',
      body: JSON.stringify(params),
    }),

  deleteSubscription: (entityId: string, subscriptionId: string) =>
    apiFetch<void>(`/api/entities/${entityId}/subscriptions/${subscriptionId}`, { method: 'DELETE' }),
}
