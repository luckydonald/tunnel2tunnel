<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import EntityName from '@/components/EntityName.vue'
import PubkeyInput, { type ParsedKey } from '@/components/PubkeyInput.vue'
import SshCommandDisplay from '@/components/SshCommandDisplay.vue'
import { entitiesApi, type EntityDetail, type EntityPort } from '@/api/entities'
import { friendsApi, type AccessRule } from '@/api/friends'
import { adminApi, type ConnLog } from '@/api/admin'
import { subjectTypeLabel, subjectTypeOptions } from '@/labels'

const route = useRoute()
const router = useRouter()
const entityId = route.params.id as string

const entity = ref<EntityDetail | null>(null)
const loading = ref(true)
const pageError = ref<string | null>(null)

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

// Port form
const showAddPort = ref(false)
const newPort = ref({ enabled: true, local_port: 8080, proxy_port: 8080, name: '', sort_order: 0 })
const addingPort = ref(false)

function defaultFilename(name: string | null, type: string): string {
  const base = name ?? type
  return 't2t_' + base.toLowerCase().replace(/[^a-z0-9_-]+/g, '_').replace(/^_+|_+$/g, '')
}

async function load(): Promise<void> {
  try {
    entity.value = await entitiesApi.getEntity(entityId)
    if (!keyFilename.value) {
      keyFilename.value = defaultFilename(entity.value.name, entity.value.entity_type)
    }
  } catch (e) {
    pageError.value = e instanceof Error ? e.message : 'Failed to load entity'
  } finally {
    loading.value = false
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

async function handleDeleteKey(keyId: string): Promise<void> {
  if (!confirm('Delete this key?')) return
  try {
    await entitiesApi.deleteKey(entityId, keyId)
    if (entity.value) entity.value.ssh_keys = entity.value.ssh_keys.filter(k => k.id !== keyId)
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to delete key')
  }
}

async function handleAddPort(): Promise<void> {
  if (!entity.value) return
  addingPort.value = true
  try {
    const port = await entitiesApi.createPort(entityId, {
      ...newPort.value,
      name: newPort.value.name || null,
    })
    entity.value.ports.push(port)
    showAddPort.value = false
    newPort.value = { enabled: true, local_port: 8080, proxy_port: 8080, name: '', sort_order: 0 }
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to add port')
  } finally {
    addingPort.value = false
  }
}

async function handleUpdatePort(port: EntityPort): Promise<void> {
  try {
    const updated = await entitiesApi.updatePort(entityId, port.id, {
      enabled: port.enabled,
      local_port: port.local_port,
      proxy_port: port.proxy_port,
      name: port.name,
      description: port.description,
      sort_order: port.sort_order,
    })
    if (entity.value) {
      const idx = entity.value.ports.findIndex(p => p.id === port.id)
      if (idx >= 0) entity.value.ports[idx] = updated
    }
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to update port')
  }
}

async function handleDeletePort(portId: string): Promise<void> {
  if (!confirm('Delete this port?')) return
  try {
    await entitiesApi.deletePort(entityId, portId)
    if (entity.value) entity.value.ports = entity.value.ports.filter(p => p.id !== portId)
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to delete port')
  }
}

// Access rules
const accessRules = ref<AccessRule[]>([])
const accessLoaded = ref(false)
const showAddAccess = ref(false)
const newAccess = ref({ subject_type: 'public_lite', hostname: '' })
const addingAccess = ref(false)

async function loadAccess(): Promise<void> {
  if (accessLoaded.value) return
  try {
    accessRules.value = await friendsApi.listAccess(entityId)
    accessLoaded.value = true
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to load access rules')
  }
}

async function handleAddAccess(): Promise<void> {
  addingAccess.value = true
  try {
    const rule = await friendsApi.createAccess(entityId, {
      subject_type: newAccess.value.subject_type,
      hostname: newAccess.value.hostname || null,
    })
    accessRules.value.push(rule)
    showAddAccess.value = false
    newAccess.value = { subject_type: 'public_lite', hostname: '' }
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to add access rule')
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
    alert(e instanceof Error ? e.message : 'Failed to delete rule')
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
    alert(e instanceof Error ? e.message : 'Failed to load logs')
  } finally {
    logsLoading.value = false
  }
}

async function handleDeleteEntity(): Promise<void> {
  if (!confirm('Delete this entity? All SSH keys and ports will also be removed.')) return
  try {
    await entitiesApi.deleteEntity(entityId)
    await router.push({ name: 'entities' })
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to delete entity')
  }
}
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
            <RouterLink :to="{ name: entity.entity_type === 'server' ? 'servers' : 'clients' }">
              {{ entity.entity_type === 'server' ? 'Servers' : 'Clients' }}
            </RouterLink>
            <span class="sep">/</span>
            <EntityName :entity="entity" />
          </div>
          <h1><EntityName :entity="entity" /></h1>
          <p v-if="entity.description" class="subtitle">{{ entity.description }}</p>
        </div>
        <button class="btn-del" @click="handleDeleteEntity">Delete entity</button>
      </div>

      <!-- SSH command + ports overview -->
      <section class="section">
        <h2>SSH Command</h2>
        <SshCommandDisplay :entity="entity" :ports="entity.ports" :filename="keyFilename" />
      </section>

      <!-- Ports management -->
      <section class="section">
        <div class="section-header">
          <h2>Ports</h2>
          <button class="btn-secondary" @click="showAddPort = true">Add port</button>
        </div>

        <table v-if="entity.ports.length" class="data-table">
          <thead>
            <tr>
              <th>Enabled</th>
              <th>Local port</th>
              <th>Proxy port</th>
              <th>Name</th>
              <th>Order</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="port in entity.ports" :key="port.id">
              <td>
                <input
                  type="checkbox"
                  :checked="port.enabled"
                  @change="port.enabled = ($event.target as HTMLInputElement).checked; handleUpdatePort(port)"
                />
              </td>
              <td>
                <input
                  type="number" class="port-num" :value="port.local_port" min="1" max="65535"
                  @blur="port.local_port = +($event.target as HTMLInputElement).value; handleUpdatePort(port)"
                />
              </td>
              <td>
                <input
                  type="number" class="port-num" :value="port.proxy_port" min="1" max="65535"
                  @blur="port.proxy_port = +($event.target as HTMLInputElement).value; handleUpdatePort(port)"
                />
              </td>
              <td>
                <input
                  type="text" class="port-name" :value="port.name ?? ''"
                  @blur="port.name = ($event.target as HTMLInputElement).value || null; handleUpdatePort(port)"
                />
              </td>
              <td>
                <input
                  type="number" class="port-order" :value="port.sort_order" min="0"
                  @blur="port.sort_order = +($event.target as HTMLInputElement).value; handleUpdatePort(port)"
                />
              </td>
              <td><button class="btn-del-sm" @click="handleDeletePort(port.id)">×</button></td>
            </tr>
          </tbody>
        </table>
        <p v-else class="empty">No ports configured.</p>

        <!-- Add port form (inline) -->
        <div v-if="showAddPort" class="add-port-form">
          <input v-model.number="newPort.local_port" type="number" class="port-num" placeholder="Local" min="1" max="65535" />
          <input v-model.number="newPort.proxy_port" type="number" class="port-num" placeholder="Proxy" min="1" max="65535" />
          <input v-model="newPort.name" type="text" class="port-name" placeholder="Name (optional)" />
          <label class="checkbox-label">
            <input v-model="newPort.enabled" type="checkbox" /> Enabled
          </label>
          <button class="btn-primary" :disabled="addingPort" @click="handleAddPort">
            {{ addingPort ? 'Adding…' : 'Add' }}
          </button>
          <button class="btn-secondary" @click="showAddPort = false">Cancel</button>
        </div>
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
              <td>{{ key.name ?? '—' }}</td>
              <td class="td-desc">{{ key.comment ?? '—' }}</td>
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
          <button class="btn-secondary" @click="showAddAccess = !showAddAccess; loadAccess()">
            {{ showAddAccess ? 'Cancel' : 'Add rule' }}
          </button>
        </div>

        <!-- lazy-load on first open -->
        <template v-if="accessLoaded">
          <div v-if="showAddAccess" class="add-access-form">
            <label class="field-label">Subject type</label>
            <select v-model="newAccess.subject_type" class="select-sm">
              <option v-for="opt in subjectTypeOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>
            <input
              v-model="newAccess.hostname"
              type="text"
              class="input-sm"
              placeholder="Hostname alias (optional)"
            />
            <button class="btn-primary" :disabled="addingAccess" @click="handleAddAccess">
              {{ addingAccess ? 'Adding…' : 'Add' }}
            </button>
          </div>

          <table v-if="accessRules.length" class="data-table">
            <thead>
              <tr>
                <th>Subject type</th>
                <th>Subject entity</th>
                <th>Subject user</th>
                <th>Hostname alias</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="rule in accessRules" :key="rule.id">
                <td>{{ subjectTypeLabel[rule.subject_type] }}</td>
                <td><code v-if="rule.subject_entity_id" class="fp">{{ rule.subject_entity_id }}</code><span v-else>—</span></td>
                <td><code v-if="rule.subject_user_id" class="fp">{{ rule.subject_user_id }}</code><span v-else>—</span></td>
                <td>{{ rule.hostname ?? '—' }}</td>
                <td><button class="btn-del-sm" @click="handleDeleteAccess(rule.id)">×</button></td>
              </tr>
            </tbody>
          </table>
          <p v-else-if="!showAddAccess" class="empty">No access rules.</p>
        </template>
        <p v-else class="empty">Click "Add rule" to manage access.</p>
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
              <th>Peer IP</th>
              <th>Fingerprint</th>
              <th>Result</th>
              <th>Reason</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="l in connLogs" :key="l.id">
              <td class="td-ts">{{ new Date(l.started_at).toLocaleString() }}</td>
              <td>{{ l.peer_ip ?? '—' }}</td>
              <td><code v-if="l.key_fingerprint" class="fp">{{ l.key_fingerprint }}</code><span v-else>—</span></td>
              <td>
                <span :class="['badge-result', l.login_succeeded ? 'ok' : 'fail']">
                  {{ l.login_succeeded ? 'ok' : 'fail' }}
                </span>
              </td>
              <td class="td-desc">{{ l.failure_reason ?? '—' }}</td>
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

.breadcrumb {
  font-size: 0.8125rem; color: #64748b;
  a { color: #94a3b8; text-decoration: none; &:hover { color: #e2e8f0; } }
  .sep { margin: 0 0.375rem; }
}

.subtitle { margin: 0.25rem 0 0; color: #94a3b8; font-size: 0.9375rem; }

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

.port-num   { width: 70px; }
.port-name  { width: 120px; }
.port-order { width: 55px; }

.port-num, .port-name, .port-order {
  padding: 0.25rem 0.375rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 3px; color: #e2e8f0; font-size: 0.875rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.add-port-form {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
  padding: 0.75rem; background: #1a1d27; border-radius: 6px; margin-top: 0.75rem;
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

.input-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.875rem; min-width: 180px;
  &:focus { outline: none; border-color: #4f6ef7; }
}
</style>
