export interface ServerInfo {
  ssh_port: number
  ssh_port_env_var: string
  ssh_host_key_fingerprint: string
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
  return res.json() as Promise<T>
}

export const serverApi = {
  getInfo: () => apiFetch<ServerInfo>('/api/server-info'),
}
