// Unified live-connections shape pushed over `/api/live-connections/ws` —
// replaces the former per-entity/`me`/admin response types. One entry per
// entity the connection's current scope includes (just the viewer's own
// entities by default, or every entity system-wide once an admin switches
// to `scope: 'all'`).

import type { RemoteStatus } from '@/liveStatus'

export interface LiveAccountRef {
  user_id: string
  username: string
}

export interface LiveEntityRef {
  id: string
  name: string | null
}

export interface LiveSubscriberInfo {
  entity: LiveEntityRef
  account: LiveAccountRef
  peer_ip: string
  connected_since: string
}

export interface ServiceLiveStatus {
  port_config_id: string
  service_name: string
  proxy_port: number
  /** Aggregated over current subscribers — see `live_connections.rs`'s module doc comment. */
  live: boolean
  /** This entity's own SSH-session/port state — see `live_connections.rs`'s module doc comment. */
  remote_status: RemoteStatus | null
  subscribers: LiveSubscriberInfo[]
}

export interface SubscriptionLiveStatus {
  subscription_id: string
  port_config_id: string
  owner: LiveEntityRef
  service_name: string
  proxy_port: number
  subscriber_local_port: number
  enabled: boolean
  live: boolean
  remote_status: RemoteStatus | null
  /** This subscriber's own bridge peer address — always this viewer's own connection. */
  peer_ip: string | null
  connected_since: string | null
}

export interface EntityLiveSnapshot {
  entity_id: string
  entity_name: string | null
  entity_online: boolean
  entity_last_disconnected_at: string | null
  /** Owned by the requesting user. Always `true` in `scope: 'mine'`; only
   * meaningfully `false` for other users' entities while an admin is in
   * `scope: 'all'`. */
  mine: boolean
  services: ServiceLiveStatus[]
  subscriptions: SubscriptionLiveStatus[]
  /** Admin-only — present only while the connection is in `scope: 'all'`. */
  account?: LiveAccountRef
}

export type LiveEventReason =
  | { kind: 'entity_online'; entity_id: string }
  | { kind: 'entity_offline'; entity_id: string }
  | { kind: 'port_forwarding_started'; entity_id: string; proxy_port: number }
  | { kind: 'port_forwarding_stopped'; entity_id: string; proxy_port: number }
  | { kind: 'bridge_started'; client_entity_id: string; target_entity_id: string; proxy_port: number }
  | { kind: 'bridge_stopped'; client_entity_id: string; target_entity_id: string; proxy_port: number }

export interface LiveConnectionsMessage {
  data: EntityLiveSnapshot[]
  /** Absent for a plain resync (initial connect / fallback interval / lagged
   * event receiver / scope switch) — only a present `reason` is toast-worthy. */
  reason?: LiveEventReason
}

export type LiveScope = 'mine' | 'all'
