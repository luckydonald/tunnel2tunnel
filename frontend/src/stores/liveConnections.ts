import { ref } from 'vue'
import { defineStore } from 'pinia'
import { createLiveSocket } from '@/composables/createLiveSocket'
import { useToast } from '@/composables/useToast'
import type {
  EntityLiveSnapshot,
  LiveConnectionsMessage,
  LiveEventReason,
  LiveScope,
} from '@/api/liveConnections'

/**
 * Owns the single app-wide connection to `/api/live-connections/ws`. Lives
 * for the whole session (connected/disconnected from `App.vue` based on
 * auth state, not tied to any page's mount/unmount) so toasts fire and the
 * dashboard/entity-detail/admin pages all stay in sync regardless of
 * navigation — see `ai/plans` for the full design rationale.
 */
export const useLiveConnectionsStore = defineStore('liveConnections', () => {
  const snapshots = ref<EntityLiveSnapshot[]>([])
  const connected = ref(false)
  const scope = ref<LiveScope>('mine')

  const { show: toast } = useToast()

  let socket: { close: () => void; send: (data: unknown) => void } | null = null
  const entityNameById = new Map<string, string>()
  const serviceNameByKey = new Map<string, string>()
  let myEntityIds = new Set<string>()

  function rebuildLookups(data: EntityLiveSnapshot[]): void {
    entityNameById.clear()
    serviceNameByKey.clear()
    myEntityIds = new Set()
    for (const snap of data) {
      entityNameById.set(snap.entity_id, snap.entity_name ?? shortId(snap.entity_id))
      if (snap.mine) myEntityIds.add(snap.entity_id)
      for (const svc of snap.services) {
        serviceNameByKey.set(`${snap.entity_id}:${svc.proxy_port}`, svc.service_name)
      }
      for (const sub of snap.subscriptions) {
        serviceNameByKey.set(`${snap.entity_id}:${sub.subscriber_local_port}`, sub.service_name)
        // The subscription row already knows the *owner's* real port/name —
        // register it too, so a bridge event naming the owner (who may not
        // be one of "my" entities and thus have no snapshot of their own)
        // still resolves to a friendly service name.
        serviceNameByKey.set(`${sub.owner.id}:${sub.proxy_port}`, sub.service_name)
        if (sub.owner.name && !entityNameById.has(sub.owner.id)) {
          entityNameById.set(sub.owner.id, sub.owner.name)
        }
      }
    }
  }

  function shortId(id: string): string {
    return id.slice(0, 8) + '…'
  }

  function nameFor(entityId: string): string {
    return entityNameById.get(entityId) ?? shortId(entityId)
  }

  function serviceFor(entityId: string, port: number): string {
    return serviceNameByKey.get(`${entityId}:${port}`) ?? `port ${port}`
  }

  function handleReason(reason: LiveEventReason): void {
    switch (reason.kind) {
      case 'entity_online':
        if (myEntityIds.has(reason.entity_id)) {
          toast(`${nameFor(reason.entity_id)} is online`, 'success')
        }
        break
      case 'entity_offline':
        if (myEntityIds.has(reason.entity_id)) {
          toast(`${nameFor(reason.entity_id)} went offline`, 'info')
        }
        break
      case 'port_forwarding_started':
        if (myEntityIds.has(reason.entity_id)) {
          toast(`${serviceFor(reason.entity_id, reason.proxy_port)} on ${nameFor(reason.entity_id)} started forwarding`, 'info')
        }
        break
      case 'port_forwarding_stopped':
        if (myEntityIds.has(reason.entity_id)) {
          toast(`${serviceFor(reason.entity_id, reason.proxy_port)} on ${nameFor(reason.entity_id)} stopped forwarding`, 'info')
        }
        break
      case 'bridge_started':
      case 'bridge_stopped': {
        const mineAsClient = myEntityIds.has(reason.client_entity_id)
        const mineAsTarget = myEntityIds.has(reason.target_entity_id)
        if (!mineAsClient && !mineAsTarget) break
        const service = serviceFor(reason.target_entity_id, reason.proxy_port)
        const verb = reason.kind === 'bridge_started' ? 'Connected to' : 'Disconnected from'
        const level = reason.kind === 'bridge_started' ? 'success' : 'info'
        if (mineAsClient) {
          toast(`${verb} ${service} on ${nameFor(reason.target_entity_id)}`, level)
        } else {
          const clientVerb = reason.kind === 'bridge_started' ? 'connected to' : 'disconnected from'
          toast(`${nameFor(reason.client_entity_id)} ${clientVerb} ${service}`, level)
        }
        break
      }
    }
  }

  function handleMessage(msg: LiveConnectionsMessage): void {
    snapshots.value = msg.data
    connected.value = true
    rebuildLookups(msg.data)
    if (msg.reason) handleReason(msg.reason)
  }

  function connect(): void {
    if (socket) return
    socket = createLiveSocket<LiveConnectionsMessage>('/api/live-connections/ws', handleMessage)
  }

  function disconnect(): void {
    socket?.close()
    socket = null
    connected.value = false
    snapshots.value = []
    scope.value = 'mine'
  }

  /** Also used as a generic "resync now" request (e.g. right after an HTTP
   * action the SSH-side event stream doesn't cover) — re-sending the
   * connection's current scope still forces the server to rebuild+resend. */
  function setScope(next: LiveScope): void {
    scope.value = next
    socket?.send({ cmd: 'set_scope', scope: next })
  }

  return { snapshots, connected, scope, connect, disconnect, setScope }
})
