<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import EntityName from '@/components/EntityName.vue'
import { useEntitiesStore } from '@/stores/entities'
import { useToast } from '@/composables/useToast'

const { show: toast } = useToast()

const props = defineProps<{
  entityType?: 'server' | 'client'
}>()

const router = useRouter()
const store = useEntitiesStore()

const title = computed(() =>
  props.entityType === 'server' ? 'Servers'
  : props.entityType === 'client' ? 'Clients'
  : 'All Entities',
)

// Create form
const showCreate = ref(false)
const newName = ref('')
const newDescription = ref('')
const createError = ref<string | null>(null)
const creating = ref(false)

async function handleCreate(): Promise<void> {
  createError.value = null
  creating.value = true
  try {
    const entity = await store.createEntity({
      entity_type: props.entityType ?? 'server',
      name: newName.value || null,
      description: newDescription.value || null,
    })
    showCreate.value = false
    newName.value = ''
    newDescription.value = ''
    await router.push({ name: 'entity-detail', params: { id: entity.id } })
  } catch (e) {
    createError.value = e instanceof Error ? e.message : 'Failed to create'
  } finally {
    creating.value = false
  }
}

async function handleDelete(id: string): Promise<void> {
  if (!confirm('Delete this entity? This cannot be undone.')) return
  try {
    await store.deleteEntity(id)
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to delete')
  }
}

onMounted(() => store.fetchEntities(props.entityType))
watch(() => props.entityType, (t) => store.fetchEntities(t))
</script>

<template>
  <AppShell>
    <div class="page-header">
      <h1>{{ title }}</h1>
      <button class="btn-primary" @click="showCreate = true">New</button>
    </div>

    <div v-if="store.loading" class="loading">Loading…</div>
    <div v-else-if="store.error" class="error-msg">{{ store.error }}</div>
    <template v-else>
      <table v-if="store.entities.length" class="data-table">
        <thead>
          <tr>
            <th>Name / ID</th>
            <th>Status</th>
            <th v-if="!entityType">Type</th>
            <th>Description</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in store.entities" :key="e.id">
            <td>
              <RouterLink :to="{ name: 'entity-detail', params: { id: e.id } }">
                <EntityName :entity="e" />
              </RouterLink>
            </td>
            <td>
              <span :class="['badge-online', e.online ? 'online' : 'offline']">
                {{ e.online ? 'Online' : 'Offline' }}
              </span>
            </td>
            <td v-if="!entityType" class="td-badge">
              <span :class="['badge', e.entity_type]">{{ e.entity_type }}</span>
            </td>
            <td class="td-desc">{{ e.description ?? '—' }}</td>
            <td class="td-actions">
              <button class="btn-del" @click="handleDelete(e.id)">Delete</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">No {{ title.toLowerCase() }} yet.</p>
    </template>

    <!-- Create modal -->
    <div v-if="showCreate" class="modal-overlay" @click.self="showCreate = false">
      <div class="modal">
        <h2>New {{ entityType ?? 'entity' }}</h2>
        <form @submit.prevent="handleCreate">
          <div class="field" v-if="!entityType">
            <label>Type</label>
            <!-- type is fixed if route is /servers or /clients -->
          </div>
          <div class="field">
            <label>Name <span class="optional">(optional)</span></label>
            <input v-model="newName" type="text" placeholder="My server" />
          </div>
          <div class="field">
            <label>Description <span class="optional">(optional)</span></label>
            <input v-model="newDescription" type="text" />
          </div>
          <p v-if="createError" class="error-msg">{{ createError }}</p>
          <div class="modal-actions">
            <button type="button" class="btn-secondary" @click="showCreate = false">Cancel</button>
            <button type="submit" class="btn-primary" :disabled="creating">
              {{ creating ? 'Creating…' : 'Create' }}
            </button>
          </div>
        </form>
      </div>
    </div>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 1.5rem;

  h1 { margin: 0; }
}

.data-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.9375rem;

  th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #2d3248;
    color: #64748b;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  td {
    padding: 0.625rem 0.75rem;
    border-bottom: 1px solid #1e2235;
    color: #e2e8f0;
  }

  a { color: #7dd3fc; text-decoration: none; &:hover { text-decoration: underline; } }
}

.td-desc { color: #64748b; font-size: 0.875rem; }

.badge {
  display: inline-block;
  padding: 0.15em 0.5em;
  border-radius: 4px;
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;

  &.server { background: rgba(79, 110, 247, 0.2); color: #93c5fd; }
  &.client { background: rgba(52, 211, 153, 0.2); color: #6ee7b7; }
}

.badge-online {
  display: inline-block; padding: 0.15em 0.5em; border-radius: 4px;
  font-size: 0.6875rem; font-weight: 600; text-transform: uppercase;
  &.online  { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.offline { background: rgba(100,116,139,.15); color: #94a3b8; }
}

.td-actions { text-align: right; }
.btn-del {
  background: none; border: 1px solid #3f1e1e; border-radius: 4px;
  color: #f87171; padding: 0.25rem 0.5rem; font-size: 0.75rem; cursor: pointer;
  &:hover { background: rgba(239, 68, 68, 0.1); }
}

.empty { color: #64748b; }
.loading { color: #94a3b8; }
.error-msg { color: #fca5a5; }

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

.btn-primary {
  padding: .5rem 1.25rem; background: #4f6ef7; border: none; border-radius: 4px;
  color: #fff; font-size: .9375rem; cursor: pointer;
  &:hover:not(:disabled) { background: #3d5ce5; }
  &:disabled { opacity: .6; cursor: not-allowed; }
}
.btn-secondary {
  padding: .5rem 1.25rem; background: none; border: 1px solid #2d3248; border-radius: 4px;
  color: #94a3b8; font-size: .9375rem; cursor: pointer;
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
}
</style>
