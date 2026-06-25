<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import EntityName from '@/components/EntityName.vue'
import PubkeyInput, { type ParsedKey } from '@/components/PubkeyInput.vue'
import SshCommandDisplay from '@/components/SshCommandDisplay.vue'
import { entitiesApi, type EntityDetail, type EntityPort } from '@/api/entities'

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

// Port form
const showAddPort = ref(false)
const newPort = ref({ enabled: true, local_port: 8080, proxy_port: 8080, name: '', sort_order: 0 })
const addingPort = ref(false)

async function load(): Promise<void> {
  try {
    entity.value = await entitiesApi.getEntity(entityId)
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
        <SshCommandDisplay :entity="entity" :ports="entity.ports" />
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
          <PubkeyInput v-model="parsedKey" />
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

      <!-- Connection log placeholder -->
      <section class="section">
        <h2>Connection Log</h2>
        <p class="empty">Connection logging available in Phase 4.</p>
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
</style>
