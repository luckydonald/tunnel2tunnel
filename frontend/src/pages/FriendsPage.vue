<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import AppShell from '@/components/AppShell.vue'
import { useAuthStore } from '@/stores/auth'
import { useFriendsStore } from '@/stores/friends'
import { friendsApi, type EntityGrant } from '@/api/friends'
import { useEntitiesStore } from '@/stores/entities'
import type { Friendship } from '@/api/friends'

const auth = useAuthStore()
const store = useFriendsStore()
const entitiesStore = useEntitiesStore()

const addUsername = ref('')
const addError = ref<string | null>(null)
const adding = ref(false)

// per-friendship expanded state for entity grants
const expandedId = ref<string | null>(null)
const grants = ref<EntityGrant[]>([])
const grantsLoading = ref(false)
const grantEntityId = ref('')

onMounted(() => {
  store.fetchFriends()
  entitiesStore.fetchEntities()
})

async function handleSendRequest(): Promise<void> {
  if (!addUsername.value.trim()) return
  addError.value = null
  adding.value = true
  try {
    await store.sendRequest(addUsername.value.trim())
    addUsername.value = ''
  } catch (e) {
    addError.value = e instanceof Error ? e.message : 'Failed'
  } finally {
    adding.value = false
  }
}

async function handleUpdate(
  f: Friendship,
  patch: { status?: string; visibility_grant?: string },
): Promise<void> {
  try {
    await store.updateFriendship(f.id, patch)
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to update')
  }
}

async function toggleExpand(id: string): Promise<void> {
  if (expandedId.value === id) {
    expandedId.value = null
    return
  }
  expandedId.value = id
  grantsLoading.value = true
  try {
    grants.value = await friendsApi.listGrants(id)
  } finally {
    grantsLoading.value = false
  }
}

async function handleAddGrant(): Promise<void> {
  if (!expandedId.value || !grantEntityId.value) return
  try {
    const g = await friendsApi.addGrant(expandedId.value, grantEntityId.value)
    grants.value.push(g)
    grantEntityId.value = ''
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to add grant')
  }
}

async function handleRemoveGrant(entity_id: string): Promise<void> {
  if (!expandedId.value) return
  try {
    await friendsApi.removeGrant(expandedId.value, entity_id)
    grants.value = grants.value.filter(g => g.entity_id !== entity_id)
  } catch (e) {
    alert(e instanceof Error ? e.message : 'Failed to remove grant')
  }
}

function friendName(f: Friendship): string {
  return f.from_user_id === auth.user?.id ? `→ ${f.to_user_id}` : `← ${f.from_user_id}`
}

function isOutgoing(f: Friendship): boolean {
  return f.from_user_id === auth.user?.id
}

const myEntities = computed(() => entitiesStore.entities)
</script>

<template>
  <AppShell>
    <div class="page-header">
      <h1>Friends</h1>
    </div>

    <!-- Send request -->
    <div class="add-friend-box">
      <form class="add-friend-form" @submit.prevent="handleSendRequest">
        <input
          v-model="addUsername"
          type="text"
          class="input"
          placeholder="Username"
        />
        <button type="submit" class="btn-primary" :disabled="adding || !addUsername.trim()">
          {{ adding ? 'Sending…' : 'Add friend' }}
        </button>
      </form>
      <p v-if="addError" class="error-msg">{{ addError }}</p>
    </div>

    <div v-if="store.loading" class="loading">Loading…</div>
    <div v-else-if="store.error" class="error-msg">{{ store.error }}</div>
    <template v-else>
      <p v-if="!store.friendships.length" class="empty">No friendships yet.</p>
      <div v-else class="friend-list">
        <div v-for="f in store.friendships" :key="f.id" class="friend-card">
          <div class="friend-row">
            <div class="friend-info">
              <span class="friend-direction">{{ isOutgoing(f) ? 'To' : 'From' }}</span>
              <span class="friend-user">{{ isOutgoing(f) ? f.to_user_id : f.from_user_id }}</span>
              <span :class="['badge-status', f.status]">{{ f.status }}</span>
            </div>
            <div class="friend-actions">
              <!-- accept/decline for incoming pending -->
              <template v-if="!isOutgoing(f) && f.status === 'pending'">
                <button class="btn-accept" @click="handleUpdate(f, { status: 'accepted' })">Accept</button>
                <button class="btn-decline" @click="handleUpdate(f, { status: 'declined' })">Decline</button>
              </template>
              <!-- visibility_grant for accepted -->
              <template v-if="f.status === 'accepted' && isOutgoing(f)">
                <label class="select-label">Visible:</label>
                <select
                  :value="f.visibility_grant"
                  class="select-sm"
                  @change="handleUpdate(f, { visibility_grant: ($event.target as HTMLSelectElement).value })"
                >
                  <option value="none">None</option>
                  <option value="clients">Clients</option>
                  <option value="servers">Servers</option>
                  <option value="all">All</option>
                </select>
                <button class="btn-secondary btn-sm" @click="toggleExpand(f.id)">
                  {{ expandedId === f.id ? 'Hide grants' : 'Per-entity grants' }}
                </button>
              </template>
            </div>
          </div>

          <!-- entity grants panel -->
          <div v-if="expandedId === f.id" class="grants-panel">
            <div v-if="grantsLoading" class="loading">Loading…</div>
            <template v-else>
              <div class="grants-list">
                <div v-for="g in grants" :key="g.entity_id" class="grant-row">
                  <code class="entity-id">{{ g.entity_id }}</code>
                  <button class="btn-del-sm" @click="handleRemoveGrant(g.entity_id)">×</button>
                </div>
                <p v-if="!grants.length" class="empty">No per-entity grants.</p>
              </div>
              <div class="add-grant-form">
                <select v-model="grantEntityId" class="select-sm">
                  <option value="">-- pick entity --</option>
                  <option v-for="e in myEntities" :key="e.id" :value="e.id">
                    {{ e.name ?? e.id }} ({{ e.entity_type }})
                  </option>
                </select>
                <button
                  class="btn-secondary btn-sm"
                  :disabled="!grantEntityId"
                  @click="handleAddGrant"
                >
                  Add grant
                </button>
              </div>
            </template>
          </div>
        </div>
      </div>
    </template>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header {
  display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem;
  h1 { margin: 0; }
}

.add-friend-box {
  margin-bottom: 1.5rem;
  .add-friend-form { display: flex; gap: 0.5rem; }
}

.input {
  padding: 0.5rem 0.75rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.9375rem; min-width: 200px;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.friend-list { display: flex; flex-direction: column; gap: 0.75rem; }

.friend-card {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px; padding: 1rem;
}

.friend-row {
  display: flex; align-items: center; justify-content: space-between; gap: 1rem;
  flex-wrap: wrap;
}

.friend-info {
  display: flex; align-items: center; gap: 0.5rem;
}

.friend-direction { color: #64748b; font-size: 0.8125rem; }
.friend-user { color: #e2e8f0; font-weight: 500; }

.badge-status {
  display: inline-block; padding: 0.1em 0.45em; border-radius: 4px;
  font-size: 0.6875rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em;
  &.pending  { background: rgba(251,191,36,.15); color: #fcd34d; }
  &.accepted { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.declined { background: rgba(239,68,68,.15);  color: #fca5a5; }
}

.friend-actions {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
}

.select-label { font-size: 0.8125rem; color: #94a3b8; }

.select-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.8125rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.btn-accept {
  padding: 0.25rem 0.625rem; background: rgba(52,211,153,.1); border: 1px solid #34d399;
  border-radius: 4px; color: #6ee7b7; font-size: 0.75rem; cursor: pointer;
  &:hover { background: rgba(52,211,153,.2); }
}

.btn-decline {
  padding: 0.25rem 0.625rem; background: none; border: 1px solid #3f1e1e;
  border-radius: 4px; color: #fca5a5; font-size: 0.75rem; cursor: pointer;
  &:hover { background: rgba(239,68,68,.1); }
}

.grants-panel {
  margin-top: 0.875rem; padding-top: 0.875rem; border-top: 1px solid #2d3248;
}

.grants-list { margin-bottom: 0.5rem; }
.grant-row {
  display: flex; align-items: center; gap: 0.5rem; padding: 0.25rem 0;
}

.entity-id {
  background: #0f1117; padding: 0.1em 0.35em; border-radius: 3px;
  font-size: 0.75rem; word-break: break-all;
}

.add-grant-form { display: flex; align-items: center; gap: 0.5rem; margin-top: 0.5rem; }

.btn-del-sm {
  background: none; border: none; color: #64748b; font-size: 1rem;
  cursor: pointer; padding: 0.125rem 0.375rem; border-radius: 3px;
  &:hover { color: #f87171; background: rgba(239,68,68,.1); }
}

.btn-primary {
  padding: 0.5rem 1.25rem; background: #4f6ef7; border: none; border-radius: 4px;
  color: #fff; font-size: 0.9375rem; cursor: pointer;
  &:hover:not(:disabled) { background: #3d5ce5; }
  &:disabled { opacity: 0.6; cursor: not-allowed; }
}

.btn-secondary {
  padding: 0.375rem 0.875rem; background: none; border: 1px solid #2d3248;
  border-radius: 4px; color: #94a3b8; font-size: 0.875rem; cursor: pointer;
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
  &:disabled { opacity: 0.6; cursor: not-allowed; }
  &.btn-sm { padding: 0.25rem 0.625rem; font-size: 0.8125rem; }
}

.loading { color: #94a3b8; }
.error-msg { color: #fca5a5; }
.empty { color: #64748b; margin: 0; }
</style>
