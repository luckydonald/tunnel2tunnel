<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useRoute, useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import EntityName from '@/components/EntityName.vue'
import PubkeyInput, { type ParsedKey } from '@/components/PubkeyInput.vue'
import SshCommandDisplay, { type OwnSubscriptionRow } from '@/components/SshCommandDisplay.vue'
import ServiceConnector from '@/components/ServiceConnector.vue'
import StatusDot from '@/components/StatusDot.vue'
import {
  entitiesApi,
  type Entity,
  type EntityDetail,
  type PortConfig,
  type SubscribableOwner,
} from '@/api/entities'
import { friendsApi, type AccessRule, type Friendship } from '@/api/friends'
import { adminApi, type ConnLog } from '@/api/admin'
import { subjectTypeLabel, subjectTypeOptions, failReasonLabel, tarpitMethodLabel, roleBadges } from '@/labels'
import { guessServiceName } from '@/portNames'
import { formatSince, type RemoteStatus } from '@/liveStatus'
import { useAuthStore } from '@/stores/auth'
import { useToast } from '@/composables/useToast'
import { useLiveConnectionsStore } from '@/stores/liveConnections'

const { show: toast } = useToast()
const liveConnections = useLiveConnectionsStore()
const { snapshots } = storeToRefs(liveConnections)

const route = useRoute()
const router = useRouter()
const entityId = route.params.id as string

const entity = ref<EntityDetail | null>(null)
const loading = ref(true)
const pageError = ref<string | null>(null)

const subscribableOwners = ref<SubscribableOwner[]>([])

// ── Live connections ─────────────────────────────────────────────────────────
// Read straight from the shared app-wide store — already synced regardless
// of which page is open, so this is instant on navigation (no fetch, no
// loading flash) instead of opening its own WebSocket.
const expandedServiceId = ref<string | null>(null)

const entitySnapshot = computed(() => snapshots.value.find(s => s.entity_id === entityId))
const serviceLiveStatus = computed(() => entitySnapshot.value?.services ?? [])
const subscriptionLiveStatus = computed(() => entitySnapshot.value?.subscriptions ?? [])

const serviceStatusMap = computed(() => {
  const m = new Map<string, typeof serviceLiveStatus.value[number]>()
  for (const s of serviceLiveStatus.value) m.set(s.port_config_id, s)
  return m
})

// The service row's ring shows the *other* side (subscribers), not this
// entity's own state (that's the dot — see `self-status` above). The
// snapshot only lists subscribers with an active bridge right now, so
// "idle but connected" can't be distinguished here — active/offline is
// all this data supports.
function subscriberRingStatus(portId: string): RemoteStatus | null {
  const s = serviceStatusMap.value.get(portId)
  if (!s) return null
  return s.subscribers.length > 0 ? 'active' : 'offline'
}

const subscriptionStatusMap = computed(() => {
  const m = new Map<string, typeof subscriptionLiveStatus.value[number]>()
  for (const s of subscriptionLiveStatus.value) m.set(s.subscription_id, s)
  return m
})

// Keep the entity header's online badge in sync with the store too — the
// entity itself is loaded once via HTTP (`load()` below), not from the
// socket, so it needs an explicit sync point.
watch(entitySnapshot, snap => {
  if (entity.value && snap) {
    entity.value.online = snap.entity_online
    entity.value.last_disconnected_at = snap.entity_last_disconnected_at
  }
})

// Subscribe/unsubscribe go through the HTTP API, which the SSH-side event
// stream doesn't cover (see `live_ws.rs`'s fallback-interval rationale) —
// force an immediate resync afterwards instead of waiting out the fallback.
function refreshLiveConnections(): void {
  liveConnections.setScope(liveConnections.scope)
}

function toggleServiceExpanded(portId: string): void {
  expandedServiceId.value = expandedServiceId.value === portId ? null : portId
}

// SSH key form
const showAddKey = ref(false)
const parsedKey = ref<ParsedKey | null>(null)
const keyName = ref('')
const addingKey = ref(false)
const keyError = ref<string | null>(null)

// Key filename — persisted to localStorage per entity, default derived from entity name
const FILENAME_LS_KEY = `t2t_key_filename_${entityId}`
const keyFilename = ref(localStorage.getItem(FILENAME_LS_KEY) ?? '')
watch(keyFilename, v => localStorage.setItem(FILENAME_LS_KEY, v))

function defaultFilename(name: string | null): string {
  const base = name ?? entityId
  return 't2t_' + base.toLowerCase().replace(/[^a-z0-9_-]+/g, '_').replace(/^_+|_+$/g, '')
}

// ── My services (owned port_configs) ───────────────────────────────────────────

const showAddPort = ref(false)
const addingPort = ref(false)

// Auto-sync flags: Local port mirrors Port, Name mirrors guessServiceName(Port), each until
// the user edits that field directly for this form session.
const localPortTouched = ref(false)
const nameTouched = ref(false)

function blankNewPort() {
  return { enabled: true, local_port: 8080, proxy_port: 8080, name: '', description: null as string | null, sort_order: 0, host: 'localhost' }
}
const newPort = ref(blankNewPort())

function handleProxyPortInput(value: number): void {
  newPort.value.proxy_port = value
  if (!localPortTouched.value) newPort.value.local_port = value
  if (!nameTouched.value) newPort.value.name = guessServiceName(value) ?? ''
}

function handleLocalPortInput(value: number): void {
  newPort.value.local_port = value
  localPortTouched.value = true
}

function handleNameInput(value: string): void {
  newPort.value.name = value
  nameTouched.value = true
}

async function load(): Promise<void> {
  try {
    entity.value = await entitiesApi.getEntity(entityId)
    if (!keyFilename.value) {
      keyFilename.value = defaultFilename(entity.value.name)
    }
    await loadSubscribableServices()
    loadAccess()
    loadIncomingAccess()
  } catch (e) {
    pageError.value = e instanceof Error ? e.message : 'Failed to load entity'
  } finally {
    loading.value = false
  }
}

async function loadSubscribableServices(): Promise<void> {
  try {
    subscribableOwners.value = await entitiesApi.getSubscribableServices(entityId)
  } catch {
    // non-critical; My subscriptions section stays empty
  }
}

onMounted(load)

async function handleAddKey(): Promise<void> {
  if (!parsedKey.value || !entity.value) return
  addingKey.value = true
  keyError.value = null
  try {
    const key = await entitiesApi.addKey(entityId, {
      algorithm: parsedKey.value.algorithm,
      key_data: parsedKey.value.key_data,
      comment: parsedKey.value.comment,
      name: keyName.value || null,
    })
    entity.value.ssh_keys.push(key)
    showAddKey.value = false
    parsedKey.value = null
    keyName.value = ''
  } catch (e) {
    keyError.value = e instanceof Error ? e.message : 'Failed to add key'
  } finally {
    addingKey.value = false
  }
}

async function handleUpdateKey(keyId: string, params: { name: string | null; comment: string | null }): Promise<void> {
  if (!entity.value) return
  try {
    const updated = await entitiesApi.updateKey(entityId, keyId, params)
    const idx = entity.value.ssh_keys.findIndex(k => k.id === keyId)
    if (idx >= 0) entity.value.ssh_keys[idx] = updated
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to update key')
  }
}

async function handleDeleteKey(keyId: string): Promise<void> {
  if (!confirm('Delete this key?')) return
  try {
    await entitiesApi.deleteKey(entityId, keyId)
    if (entity.value) entity.value.ssh_keys = entity.value.ssh_keys.filter(k => k.id !== keyId)
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to delete key')
  }
}

async function handleAddPort(): Promise<void> {
  if (!entity.value || !newPort.value.name) return
  addingPort.value = true
  try {
    const port = await entitiesApi.createPort(entityId, { ...newPort.value })
    entity.value.ports.push(port)
    showAddPort.value = false
    newPort.value = blankNewPort()
    localPortTouched.value = false
    nameTouched.value = false
    // Roles may have just changed (first service added) — refresh the entity header.
    entity.value.is_server = true
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to add port')
  } finally {
    addingPort.value = false
  }
}

async function handleUpdatePort(port: PortConfig): Promise<void> {
  try {
    const updated = await entitiesApi.updatePort(entityId, port.id, {
      enabled: port.enabled,
      local_port: port.local_port,
      proxy_port: port.proxy_port,
      name: port.name,
      description: port.description,
      sort_order: port.sort_order,
      host: port.host,
    })
    if (entity.value) {
      const idx = entity.value.ports.findIndex(p => p.id === port.id)
      if (idx >= 0) entity.value.ports[idx] = updated
    }
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to update port')
  }
}

async function handleDeletePort(portId: string): Promise<void> {
  if (!confirm('Delete this service?')) return
  try {
    await entitiesApi.deletePort(entityId, portId)
    if (entity.value) entity.value.ports = entity.value.ports.filter(p => p.id !== portId)
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to delete service')
  }
}

// ── My subscriptions ────────────────────────────────────────────────────────────

const auth = useAuthStore()
const ownEntityIds = ref<string[]>([])

async function loadOwnEntityIds(): Promise<void> {
  try {
    const mine = await entitiesApi.list()
    ownEntityIds.value = mine.map(e => e.id)
  } catch {
    // non-critical; Origin "Mine" filter just won't narrow anything
  }
}

async function handleSubscribe(portConfigId: string, localPort: number): Promise<void> {
  try {
    await entitiesApi.createSubscription(entityId, { port_config_id: portConfigId, subscriber_local_port: localPort })
    await loadSubscribableServices()
    refreshLiveConnections()
    if (entity.value) entity.value.is_client = true
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to subscribe')
  }
}

async function handleUnsubscribe(subscriptionId: string): Promise<void> {
  try {
    await entitiesApi.deleteSubscription(entityId, subscriptionId)
    await loadSubscribableServices()
    refreshLiveConnections()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to unsubscribe')
  }
}

async function handleUpdateLocalPort(subscriptionId: string, localPort: number): Promise<void> {
  try {
    await entitiesApi.updateSubscription(entityId, subscriptionId, { subscriber_local_port: localPort })
    await loadSubscribableServices()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to update local port')
  }
}

// Flattened view of this entity's own subscriptions (for the SSH command's -L flags)
const ownSubscriptionRows = computed((): OwnSubscriptionRow[] => {
  const rows: OwnSubscriptionRow[] = []
  for (const owner of subscribableOwners.value) {
    for (const service of owner.services) {
      if (service.subscription) {
        rows.push({ subscription: service.subscription, service, ownerId: owner.id, ownerName: owner.name })
      }
    }
  }
  return rows
})

// Access rules
const accessRules = ref<AccessRule[]>([])
const accessLoaded = ref(false)
const showAddAccess = ref(false)

type NewAccess = {
  subject_type: AccessRule['subject_type']
  subject_entity_id: string | null
  subject_user_id: string | null
  hostname: string
  scope: 'whole' | 'port'
  port_config_id: string | null
}
const blankAccess = (): NewAccess => ({
  subject_type: 'public_lite',
  subject_entity_id: null,
  subject_user_id: null,
  hostname: '',
  scope: 'whole',
  port_config_id: null,
})
const newAccess = ref<NewAccess>(blankAccess())
const addingAccess = ref(false)

// Form data — entities + accepted friends, loaded once when the form is opened
const accessFormEntities = ref<Entity[]>([])
const accessFormFriends = ref<Friendship[]>([])
const accessFormDataLoaded = ref(false)

async function loadAccess(): Promise<void> {
  if (accessLoaded.value) return
  try {
    accessRules.value = await friendsApi.listAccess(entityId)
    accessLoaded.value = true
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load access rules')
  }
}

async function loadAccessFormData(): Promise<void> {
  if (accessFormDataLoaded.value) return
  try {
    const [entities, friends] = await Promise.all([
      entitiesApi.list(),
      friendsApi.listFriends(),
    ])
    accessFormEntities.value = entities
    accessFormFriends.value = friends.filter(f => f.status === 'accepted')
    accessFormDataLoaded.value = true
  } catch {
    // non-critical
  }
}

// helper to get the other user's ID from a friendship
function friendUserId(f: Friendship): string {
  return f.from_user_id === auth.user?.id ? f.to_user_id : f.from_user_id
}

// derived lookup maps for displaying in the rules table once form data is loaded
const entityNameMap = computed(() => {
  const m = new Map<string, string>()
  for (const e of accessFormEntities.value)
    m.set(e.id, e.name ? e.name : `${e.id.slice(0, 8)}…`)
  return m
})

const portConfigNameMap = computed(() => {
  const m = new Map<string, string>()
  if (entity.value) for (const p of entity.value.ports) m.set(p.id, p.name)
  return m
})

const friendNameMap = computed(() => {
  const m = new Map<string, string>()
  for (const f of accessFormFriends.value) {
    const uid = friendUserId(f)
    m.set(uid, uid.slice(0, 13) + '…')
  }
  return m
})

// reset sub-fields when type changes
watch(() => newAccess.value.subject_type, () => {
  newAccess.value.subject_entity_id = null
  newAccess.value.subject_user_id = null
})

async function handleAddAccess(): Promise<void> {
  addingAccess.value = true
  try {
    const rule = await friendsApi.createAccess(entityId, {
      subject_type: newAccess.value.subject_type,
      subject_entity_id: newAccess.value.subject_entity_id ?? null,
      subject_user_id: newAccess.value.subject_user_id ?? null,
      hostname: newAccess.value.hostname || null,
      port_config_id: newAccess.value.scope === 'port' ? newAccess.value.port_config_id : null,
    })
    accessRules.value.push(rule)
    showAddAccess.value = false
    newAccess.value = blankAccess()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to add access rule')
  } finally {
    addingAccess.value = false
  }
}

async function handleDeleteAccess(ruleId: string): Promise<void> {
  if (!confirm('Delete this access rule?')) return
  try {
    await friendsApi.deleteAccess(entityId, ruleId)
    accessRules.value = accessRules.value.filter(r => r.id !== ruleId)
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to delete rule')
  }
}

// Incoming grants (rules on OTHER entities that grant access to this one)
const incomingGrants = ref<AccessRule[]>([])
const incomingGrantsLoaded = ref(false)

async function loadIncomingAccess(): Promise<void> {
  if (incomingGrantsLoaded.value) return
  try {
    incomingGrants.value = await friendsApi.listIncomingAccess(entityId)
    incomingGrantsLoaded.value = true
  } catch {
    // non-critical; section stays hidden
  }
}

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

// Edit entity (name/description)
const showEditEntity = ref(false)
const editName = ref('')
const editDescription = ref('')
const editingEntity = ref(false)
const editEntityError = ref<string | null>(null)

function openEditEntity(): void {
  if (!entity.value) return
  editName.value = entity.value.name ?? ''
  editDescription.value = entity.value.description ?? ''
  editEntityError.value = null
  showEditEntity.value = true
}

async function handleUpdateEntity(): Promise<void> {
  if (!entity.value) return
  editingEntity.value = true
  editEntityError.value = null
  try {
    const updated = await entitiesApi.update(entityId, {
      name: editName.value || null,
      description: editDescription.value || null,
    })
    entity.value.name = updated.name
    entity.value.description = updated.description
    showEditEntity.value = false
  } catch (e) {
    editEntityError.value = e instanceof Error ? e.message : 'Failed to update entity'
  } finally {
    editingEntity.value = false
  }
}

async function handleDeleteEntity(): Promise<void> {
  if (!confirm('Delete this entity? All SSH keys and services will also be removed.')) return
  try {
    await entitiesApi.deleteEntity(entityId)
    await router.push({ name: 'entities' })
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to delete entity')
  }
}

async function handleCopyDebugData(): Promise<void> {
  if (!entity.value) return
  try {
    await Promise.all([loadAccess(), loadIncomingAccess()])
    const recentLogs = await adminApi.listConnectionLogs(entityId)
    const debugData = {
      generated_at: new Date().toISOString(),
      entity: entity.value,
      live: {
        services: serviceLiveStatus.value,
        subscriptions: subscriptionLiveStatus.value,
      },
      access_rules: accessRules.value,
      incoming_grants: incomingGrants.value,
      subscribable_owners: subscribableOwners.value,
      recent_connection_logs: recentLogs,
    }
    await navigator.clipboard.writeText(JSON.stringify(debugData, null, 2))
    toast('Debug data copied to clipboard', 'success')
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to copy debug data')
  }
}

onMounted(loadOwnEntityIds)
</script>

<template>
  <AppShell>
    <div v-if="loading" class="loading">Loading…</div>
    <div v-else-if="pageError" class="error-msg">{{ pageError }}</div>
    <template v-else-if="entity">

      <!-- Header -->
      <div class="page-header">
        <div>
          <div class="breadcrumb">
            <RouterLink :to="{ name: 'entities' }">Entities</RouterLink>
            <span class="sep">/</span>
            <EntityName :entity="entity" />
          </div>
          <h1>
            <EntityName :entity="entity" />
            <span class="roles-badge">{{ roleBadges(entity) }}</span>
            <span :class="['badge-online', entity.online ? 'online' : 'offline']">
              {{ entity.online ? 'Online' : 'Offline' }}
            </span>
          </h1>
          <p v-if="!entity.online && entity.last_disconnected_at" class="subtitle">
            Last seen {{ new Date(entity.last_disconnected_at).toLocaleString() }}
          </p>
          <p v-if="entity.description" class="subtitle">{{ entity.description }}</p>
        </div>
        <div class="header-actions">
          <button class="btn-secondary" @click="openEditEntity">Edit</button>
          <button class="btn-kbd" title="Copy all known state about this entity as JSON" @click="handleCopyDebugData">
            Copy debug data
          </button>
          <button class="btn-del" @click="handleDeleteEntity">Delete entity</button>
        </div>
      </div>

      <!-- Edit entity modal -->
      <div v-if="showEditEntity" class="modal-overlay" @click.self="showEditEntity = false">
        <div class="modal">
          <h2>Edit entity</h2>
          <form @submit.prevent="handleUpdateEntity">
            <div class="field">
              <label>Name <span class="optional">(optional)</span></label>
              <input v-model="editName" type="text" placeholder="My entity" />
            </div>
            <div class="field">
              <label>Description <span class="optional">(optional)</span></label>
              <input v-model="editDescription" type="text" />
            </div>
            <p v-if="editEntityError" class="error-msg">{{ editEntityError }}</p>
            <div class="modal-actions">
              <button type="button" class="btn-secondary" @click="showEditEntity = false">Cancel</button>
              <button type="submit" class="btn-primary" :disabled="editingEntity">
                {{ editingEntity ? 'Saving…' : 'Save' }}
              </button>
            </div>
          </form>
        </div>
      </div>

      <!-- SSH command -->
      <section class="section">
        <h2>SSH Command</h2>
        <SshCommandDisplay
          :entity="entity"
          :ports="entity.ports"
          :subscriptions="ownSubscriptionRows"
          :filename="keyFilename"
        />
      </section>

      <!-- My services -->
      <section id="services" class="section">
        <div class="section-header">
          <h2>My services</h2>
          <button class="btn-secondary" @click="showAddPort = true">+ Add service</button>
        </div>

        <table v-if="entity.ports.length" class="data-table">
          <thead>
            <tr>
              <th></th>
              <th>Enabled</th>
              <th>Proxy port</th>
              <th>Local port</th>
              <th>Host</th>
              <th>Name</th>
              <th>Order</th>
              <th>Subscribers</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <template v-for="port in entity.ports" :key="port.id">
              <tr>
                <td>
                  <StatusDot
                    v-if="serviceStatusMap.get(port.id)"
                    :live="serviceStatusMap.get(port.id)!.live"
                    :self-status="serviceStatusMap.get(port.id)!.remote_status"
                    :remote-status="subscriberRingStatus(port.id)"
                    :connection-count="serviceStatusMap.get(port.id)!.subscribers.length"
                  />
                </td>
                <td>
                  <input
                    type="checkbox"
                    :checked="port.enabled"
                    @change="port.enabled = ($event.target as HTMLInputElement).checked; handleUpdatePort(port)"
                  />
                </td>
                <td>
                  <input
                    type="number" class="port-num" :value="port.proxy_port" min="1" max="65535" list="common-ports"
                    @blur="port.proxy_port = +($event.target as HTMLInputElement).value; handleUpdatePort(port)"
                  />
                </td>
                <td>
                  <input
                    type="number" class="port-num" :value="port.local_port" min="1" max="65535" list="common-ports"
                    @blur="port.local_port = +($event.target as HTMLInputElement).value; handleUpdatePort(port)"
                  />
                </td>
                <td>
                  <input
                    type="text" class="port-host" :value="port.host"
                    @blur="port.host = ($event.target as HTMLInputElement).value || 'localhost'; handleUpdatePort(port)"
                  />
                </td>
                <td>
                  <input
                    type="text" class="port-name" :value="port.name"
                    @blur="port.name = ($event.target as HTMLInputElement).value || port.name; handleUpdatePort(port)"
                  />
                </td>
                <td>
                  <input
                    type="number" class="port-order" :value="port.sort_order" min="0"
                    @blur="port.sort_order = +($event.target as HTMLInputElement).value; handleUpdatePort(port)"
                  />
                </td>
                <td>
                  <button
                    v-if="serviceStatusMap.get(port.id)?.subscribers.length"
                    class="btn-subscribers"
                    @click="toggleServiceExpanded(port.id)"
                  >
                    {{ expandedServiceId === port.id ? '▾' : '▸' }}
                    {{ serviceStatusMap.get(port.id)!.subscribers.length }} connected
                  </button>
                  <span v-else class="td-desc">0</span>
                </td>
                <td><button class="btn-del-sm" @click="handleDeletePort(port.id)">×</button></td>
              </tr>
              <tr v-if="expandedServiceId === port.id && serviceStatusMap.get(port.id)?.subscribers.length" class="subscribers-row">
                <td colspan="9">
                  <ul class="subscriber-list">
                    <li v-for="sub in serviceStatusMap.get(port.id)!.subscribers" :key="sub.entity.id + sub.account.user_id">
                      {{ sub.entity.name ?? sub.entity.id.slice(0, 8) + '…' }} ({{ sub.account.username }}), {{ sub.peer_ip }}, {{ formatSince(sub.connected_since) }}
                    </li>
                  </ul>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
        <p v-else class="empty">No services configured.</p>

        <!-- Common ports suggestions for port inputs -->
        <datalist id="common-ports">
          <option value="21" label="FTP" />
          <option value="22" label="SSH" />
          <option value="25" label="SMTP" />
          <option value="80" label="HTTP" />
          <option value="443" label="HTTPS" />
          <option value="1433" label="MSSQL" />
          <option value="3000" label="Node / dev" />
          <option value="3306" label="MySQL" />
          <option value="5432" label="PostgreSQL" />
          <option value="5900" label="VNC" />
          <option value="6379" label="Redis" />
          <option value="8080" label="HTTP alt" />
          <option value="8443" label="HTTPS alt" />
          <option value="8888" label="Jupyter" />
          <option value="27017" label="MongoDB" />
        </datalist>

        <!-- Add service form (inline) -->
        <div v-if="showAddPort" class="add-port-form">
          <div class="field-group">
            <input
              :value="newPort.proxy_port"
              type="number" class="port-num" placeholder="Port" min="1" max="65535" list="common-ports"
              @input="handleProxyPortInput(+($event.target as HTMLInputElement).value)"
            />
            <span class="field-caption">Port (required)</span>
          </div>
          <div class="field-group">
            <input
              :value="newPort.local_port"
              type="number" class="port-num" placeholder="Local port" min="1" max="65535" list="common-ports"
              @input="handleLocalPortInput(+($event.target as HTMLInputElement).value)"
            />
            <span class="field-caption">Local port (mirrors Port until edited)</span>
          </div>
          <div class="field-group">
            <input
              :value="newPort.name"
              type="text" class="port-name" placeholder="Name"
              @input="handleNameInput(($event.target as HTMLInputElement).value)"
            />
            <span class="field-caption">Name (required; auto-suggested from Port until edited)</span>
          </div>
          <input v-model="newPort.host" type="text" class="port-host" placeholder="Host" />
          <label class="checkbox-label">
            <input v-model="newPort.enabled" type="checkbox" /> Enabled
          </label>
          <button class="btn-primary" :disabled="addingPort || !newPort.name" @click="handleAddPort">
            {{ addingPort ? 'Adding…' : 'Add' }}
          </button>
          <button class="btn-secondary" @click="showAddPort = false">Cancel</button>
        </div>
      </section>

      <!-- My subscriptions -->
      <section id="subscriptions" class="section">
        <h2>My subscriptions</h2>
        <ServiceConnector
          :subscribable-owners="subscribableOwners"
          :own-entity-ids="ownEntityIds"
          :subscription-status="subscriptionStatusMap"
          @subscribe="handleSubscribe"
          @unsubscribe="handleUnsubscribe"
          @update-local-port="handleUpdateLocalPort"
        />
      </section>

      <!-- SSH keys -->
      <section class="section">
        <div class="section-header">
          <h2>SSH Keys</h2>
          <button class="btn-secondary" @click="showAddKey = !showAddKey">
            {{ showAddKey ? 'Cancel' : 'Add key' }}
          </button>
        </div>

        <div v-if="showAddKey" class="add-key-form">
          <PubkeyInput v-model="parsedKey" v-model:filename="keyFilename" />
          <div class="field" style="margin-top: .75rem">
            <label>Key name <span class="optional">(optional)</span></label>
            <input v-model="keyName" type="text" placeholder="e.g. laptop" />
          </div>
          <p v-if="keyError" class="error-msg">{{ keyError }}</p>
          <button
            class="btn-primary"
            :disabled="!parsedKey || addingKey"
            style="margin-top: .75rem"
            @click="handleAddKey"
          >{{ addingKey ? 'Adding…' : 'Add key' }}</button>
        </div>

        <table v-if="entity.ssh_keys.length" class="data-table">
          <thead>
            <tr>
              <th>Algorithm</th>
              <th>Fingerprint</th>
              <th>Name</th>
              <th>Comment</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="key in entity.ssh_keys" :key="key.id">
              <td><code>{{ key.algorithm }}</code></td>
              <td><code class="fp">{{ key.fingerprint }}</code></td>
              <td>
                <input
                  type="text" class="port-name" :value="key.name ?? ''" placeholder="—"
                  @blur="handleUpdateKey(key.id, { name: ($event.target as HTMLInputElement).value || null, comment: key.comment })"
                />
              </td>
              <td>
                <input
                  type="text" class="port-name" :value="key.comment ?? ''" placeholder="—"
                  @blur="handleUpdateKey(key.id, { name: key.name, comment: ($event.target as HTMLInputElement).value || null })"
                />
              </td>
              <td><button class="btn-del-sm" @click="handleDeleteKey(key.id)">×</button></td>
            </tr>
          </tbody>
        </table>
        <p v-else class="empty">No SSH keys.</p>
      </section>

      <!-- Access rules -->
      <section class="section">
        <div class="section-header">
          <h2>Access Rules</h2>
          <button class="btn-secondary" @click="showAddAccess = !showAddAccess; loadAccess(); loadAccessFormData()">
            {{ showAddAccess ? 'Cancel' : 'Add rule' }}
          </button>
        </div>
        <p class="section-note">Who else can subscribe to my services. Your own entities always have access to each other automatically.</p>

        <!-- lazy-load on first open -->
        <template v-if="accessLoaded">
          <div v-if="showAddAccess" class="add-access-form">
            <label class="field-label">Subject type</label>
            <select v-model="newAccess.subject_type" class="select-sm">
              <option v-for="opt in subjectTypeOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>

            <!-- entity picker -->
            <template v-if="newAccess.subject_type === 'entity'">
              <label class="field-label">Entity</label>
              <select v-model="newAccess.subject_entity_id" class="select-sm">
                <option :value="null" disabled>— pick entity —</option>
                <option v-for="e in accessFormEntities" :key="e.id" :value="e.id">
                  {{ e.name ?? e.id.slice(0, 8) + '…' }} ({{ roleBadges(e) }})
                </option>
              </select>
              <span v-if="!accessFormEntities.length" class="field-hint">No entities found.</span>
            </template>

            <!-- user picker (accepted friends) -->
            <template v-if="newAccess.subject_type === 'all_user_entities'">
              <label class="field-label">Friend's user</label>
              <select v-model="newAccess.subject_user_id" class="select-sm">
                <option :value="null" disabled>— pick friend —</option>
                <option
                  v-for="f in accessFormFriends"
                  :key="f.id"
                  :value="friendUserId(f)"
                >
                  {{ friendUserId(f) }}
                </option>
              </select>
              <span v-if="!accessFormFriends.length" class="field-hint">No accepted friends.</span>
            </template>

            <!-- scope picker -->
            <label class="field-label">Scope</label>
            <select v-model="newAccess.scope" class="select-sm">
              <option value="whole">Whole entity</option>
              <option value="port" :disabled="!entity.ports.length">Specific port</option>
            </select>
            <template v-if="newAccess.scope === 'port'">
              <select v-model="newAccess.port_config_id" class="select-sm">
                <option :value="null" disabled>— pick service —</option>
                <option v-for="p in entity.ports" :key="p.id" :value="p.id">{{ p.name }}</option>
              </select>
            </template>

            <input
              v-model="newAccess.hostname"
              type="text"
              class="input-sm"
              placeholder="Hostname alias (optional)"
            />
            <button
              class="btn-primary"
              :disabled="addingAccess
                || (newAccess.subject_type === 'entity' && !newAccess.subject_entity_id)
                || (newAccess.subject_type === 'all_user_entities' && !newAccess.subject_user_id)
                || (newAccess.scope === 'port' && !newAccess.port_config_id)"
              @click="handleAddAccess"
            >
              {{ addingAccess ? 'Adding…' : 'Add' }}
            </button>
          </div>

          <table v-if="accessRules.length" class="data-table">
            <thead>
              <tr>
                <th>Subject type</th>
                <th>Subject entity</th>
                <th>Subject user</th>
                <th>Scope</th>
                <th>Hostname alias</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="rule in accessRules" :key="rule.id">
                <td>{{ subjectTypeLabel[rule.subject_type] }}</td>
                <td>
                  <RouterLink
                    v-if="rule.subject_entity_id"
                    :to="'/entities/' + rule.subject_entity_id"
                    :title="rule.subject_entity_id"
                    class="entity-link"
                  >
                    {{ entityNameMap.get(rule.subject_entity_id) ?? rule.subject_entity_id.slice(0, 13) + '…' }}
                  </RouterLink>
                  <span v-else>—</span>
                </td>
                <td>
                  <template v-if="rule.subject_user_id">
                    <code class="fp" :title="rule.subject_user_id">
                      {{ friendNameMap.get(rule.subject_user_id) ?? rule.subject_user_id }}
                    </code>
                  </template>
                  <span v-else>—</span>
                </td>
                <td>{{ rule.port_config_id ? `Only "${portConfigNameMap.get(rule.port_config_id) ?? rule.port_config_id.slice(0, 8) + '…'}"` : 'Whole entity' }}</td>
                <td>{{ rule.hostname ?? '—' }}</td>
                <td><button class="btn-del-sm" @click="handleDeleteAccess(rule.id)">×</button></td>
              </tr>
            </tbody>
          </table>
          <p v-else-if="!showAddAccess" class="empty">No access rules.</p>
        </template>
      </section>

      <!-- Incoming grants -->
      <section v-if="incomingGrantsLoaded && incomingGrants.length" class="section">
        <h2>Incoming Grants</h2>
        <p class="section-note">Other entities that have explicitly granted this entity access.</p>
        <table class="data-table">
          <thead>
            <tr>
              <th>Owner entity</th>
              <th>Hostname alias</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="rule in incomingGrants" :key="rule.id">
              <td>
                <RouterLink :to="'/entities/' + rule.owner_entity_id" class="entity-link">
                  {{ rule.owner_entity_id.slice(0, 13) }}…
                </RouterLink>
              </td>
              <td>{{ rule.hostname ?? '—' }}</td>
              <td class="td-ts">{{ new Date(rule.created_at).toLocaleString() }}</td>
            </tr>
          </tbody>
        </table>
      </section>

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
              <th>Ended</th>
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
              <td class="td-ts">{{ l.ended_at ? new Date(l.ended_at).toLocaleString() : '—' }}</td>
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

<style lang="scss" scoped>
.loading { color: #94a3b8; }
.error-msg { color: #fca5a5; }
.empty { color: #64748b; margin: 0; }
.optional { color: #64748b; }

.page-header {
  display: flex; align-items: flex-start; justify-content: space-between; margin-bottom: 2rem;
  h1 { margin: 0.25rem 0 0; font-size: 1.5rem; }
}

.header-actions { display: flex; align-items: center; gap: 0.5rem; }

.btn-kbd {
  padding: .3rem .7rem; background: #1a1d27; border: 1px solid #2d3248; border-bottom-width: 2px;
  border-radius: 5px; color: #94a3b8; font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: .8125rem; cursor: pointer;
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
  &:active { border-bottom-width: 1px; transform: translateY(1px); }
}

.breadcrumb {
  font-size: 0.8125rem; color: #64748b;
  a { color: #94a3b8; text-decoration: none; &:hover { color: #e2e8f0; } }
  .sep { margin: 0 0.375rem; }
}

.subtitle { margin: 0.25rem 0 0; color: #94a3b8; font-size: 0.9375rem; }

.roles-badge { margin-left: 0.625rem; font-size: 0.8125rem; color: #94a3b8; vertical-align: middle; }

.section {
  margin-bottom: 2.5rem;
  h2 { font-size: 1rem; color: #94a3b8; margin-bottom: 1rem; text-transform: uppercase; letter-spacing: 0.06em; }
}

.section-header {
  display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem;
  h2 { margin: 0; }
}

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

.btn-subscribers {
  background: none; border: none; color: #94a3b8; font-size: 0.8125rem;
  cursor: pointer; padding: 0;
  &:hover { color: #e2e8f0; }
}

.subscribers-row td {
  padding: 0.25rem 0.75rem 0.5rem 2.25rem;
  background: #161927;
}

.subscriber-list {
  margin: 0; padding: 0; list-style: none;
  li { font-size: 0.8125rem; color: #94a3b8; padding: 0.125rem 0; }
}

.port-num   { width: 80px; }
.port-name  { width: 120px; }
.port-order { width: 55px; }
.port-host  { width: 140px; }

.port-num, .port-name, .port-order, .port-host {
  padding: 0.25rem 0.375rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 3px; color: #e2e8f0; font-size: 0.875rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.add-port-form {
  display: flex; align-items: flex-end; gap: 0.5rem; flex-wrap: wrap;
  padding: 0.75rem; background: #1a1d27; border-radius: 6px; margin-top: 0.75rem;
}

.field-group {
  display: flex; flex-direction: column; gap: 0.25rem;
}

.field-caption {
  font-size: 0.6875rem; color: #64748b; white-space: nowrap;
}

.checkbox-label { display: flex; align-items: center; gap: 0.375rem; color: #94a3b8; font-size: 0.875rem; }

.add-key-form {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 6px;
  padding: 1rem; margin-bottom: 1rem;
}

.field {
  label { display: block; margin-bottom: 0.375rem; font-size: 0.875rem; color: #94a3b8; }
  input {
    width: 100%; padding: 0.5rem 0.75rem; background: #0f1117;
    border: 1px solid #2d3248; border-radius: 4px; color: #e2e8f0;
    font-size: 0.875rem; box-sizing: border-box;
    &:focus { outline: none; border-color: #4f6ef7; }
  }
}

.btn-del-sm {
  background: none; border: none; color: #64748b; font-size: 1rem;
  cursor: pointer; padding: 0.125rem 0.375rem; border-radius: 3px;
  &:hover { color: #f87171; background: rgba(239,68,68,.1); }
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
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
}

.btn-del {
  padding: .375rem .875rem; background: none; border: 1px solid #3f1e1e; border-radius: 4px;
  color: #f87171; font-size: .875rem; cursor: pointer;
  &:hover { background: rgba(239,68,68,.1); }
}

.add-access-form {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
  padding: 0.75rem; background: #1a1d27; border-radius: 6px; margin-bottom: 0.75rem;
}

.field-label { font-size: 0.8125rem; color: #94a3b8; white-space: nowrap; }
.field-hint  { font-size: 0.8125rem; color: #64748b; white-space: nowrap; }

.select-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.875rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.td-ts { font-size: 0.8125rem; color: #94a3b8; white-space: nowrap; }

.badge-result {
  display: inline-block; padding: 0.1em 0.45em; border-radius: 4px;
  font-size: 0.75rem; font-weight: 600; text-transform: uppercase;
  &.ok   { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.fail { background: rgba(239,68,68,.15);  color: #fca5a5; }
}

.badge-online {
  display: inline-block; margin-left: 0.625rem; padding: 0.15em 0.5em; border-radius: 4px;
  font-size: 0.6875rem; font-weight: 600; text-transform: uppercase; vertical-align: middle;
  &.online  { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.offline { background: rgba(100,116,139,.15); color: #94a3b8; }
}

.input-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.875rem; min-width: 180px;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.section-note { font-size: 0.8125rem; color: #64748b; margin: -0.5rem 0 0.75rem; }

.entity-link { color: #818cf8; text-decoration: none; &:hover { text-decoration: underline; } }

.modal-overlay {
  position: fixed; inset: 0;
  background: rgba(0,0,0,.6);
  display: flex; align-items: center; justify-content: center;
  z-index: 100;
}

.modal {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px;
  padding: 2rem; width: 100%; max-width: 440px;
  h2 { margin: 0 0 1.5rem; font-size: 1.125rem; }
}

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
