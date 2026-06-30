export interface Friendship {
  id: string
  from_user_id: string
  to_user_id: string
  status: 'pending' | 'accepted' | 'declined'
  visibility_grant: 'none' | 'clients' | 'servers' | 'all'
  created_at: string
  updated_at: string
}

export interface EntityGrant {
  id: string
  friendship_id: string
  entity_id: string
  created_at: string
}

export interface AccessRule {
  id: string
  owner_entity_id: string
  subject_type: 'entity' | 'all_mine' | 'all_user_entities' | 'public_lite'
  subject_entity_id: string | null
  subject_user_id: string | null
  hostname: string | null
  created_at: string
  updated_at: string
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

export const friendsApi = {
  listFriends: () =>
    apiFetch<Friendship[]>('/api/friends'),

  sendRequest: (username: string) =>
    apiFetch<Friendship>('/api/friends', {
      method: 'POST',
      body: JSON.stringify({ username }),
    }),

  updateFriendship: (id: string, data: { status?: string; visibility_grant?: string }) =>
    apiFetch<Friendship>(`/api/friends/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),

  listGrants: (id: string) =>
    apiFetch<EntityGrant[]>(`/api/friends/${id}/grants`),

  addGrant: (id: string, entity_id: string) =>
    apiFetch<EntityGrant>(`/api/friends/${id}/grants`, {
      method: 'POST',
      body: JSON.stringify({ entity_id }),
    }),

  removeGrant: (id: string, entity_id: string) =>
    apiFetch<void>(`/api/friends/${id}/grants/${entity_id}`, { method: 'DELETE' }),

  listAccess: (entity_id: string) =>
    apiFetch<AccessRule[]>(`/api/entities/${entity_id}/access`),

  createAccess: (
    entity_id: string,
    body: {
      subject_type: string
      subject_entity_id?: string | null
      subject_user_id?: string | null
      hostname?: string | null
    },
  ) =>
    apiFetch<AccessRule>(`/api/entities/${entity_id}/access`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  deleteAccess: (entity_id: string, rule_id: string) =>
    apiFetch<void>(`/api/entities/${entity_id}/access/${rule_id}`, { method: 'DELETE' }),

  listIncomingAccess: (entity_id: string) =>
    apiFetch<AccessRule[]>(`/api/entities/${entity_id}/access/incoming`),
}
