<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import { adminApi } from '@/api/admin'

const router = useRouter()

interface KeyRow {
  id: string
  entity_id: string
  algorithm: string
  fingerprint: string
  name: string | null
  comment: string | null
  selected: boolean
}

const keys = ref<KeyRow[]>([])
const loading = ref(true)
const error = ref<string | null>(null)
const purging = ref(false)
const confirming = ref(false)

onMounted(async () => {
  try {
    const raw = await adminApi.listMyKeys()
    keys.value = raw.map(k => ({ ...k, selected: false }))
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load'
  } finally {
    loading.value = false
  }
})

const selected = computed(() => keys.value.filter(k => k.selected))
const allSelected = computed(() => keys.value.length > 0 && keys.value.every(k => k.selected))

function toggleAll(): void {
  const next = !allSelected.value
  keys.value.forEach(k => (k.selected = next))
}

async function handlePurge(): Promise<void> {
  purging.value = true
  try {
    await adminApi.purgeKeys(selected.value.map(k => k.id))
    keys.value = keys.value.filter(k => !k.selected)
    confirming.value = false
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Purge failed'
  } finally {
    purging.value = false
  }
}
</script>

<template>
  <AppShell>
    <div class="page-header">
      <div>
        <RouterLink to="/settings" class="back-link">← Settings</RouterLink>
        <h1>Purge SSH Keys</h1>
      </div>
    </div>

    <div v-if="loading" class="loading">Loading…</div>
    <div v-else-if="error" class="error-msg">{{ error }}</div>
    <template v-else>
      <p v-if="!keys.length" class="empty">No SSH keys found.</p>
      <template v-else>
        <div class="toolbar">
          <label class="checkbox-label">
            <input type="checkbox" :checked="allSelected" @change="toggleAll" />
            Select all
          </label>
          <span class="count">{{ selected.length }} selected</span>
        </div>

        <table class="data-table">
          <thead>
            <tr>
              <th class="th-check"></th>
              <th>Algorithm</th>
              <th>Fingerprint</th>
              <th>Name</th>
              <th>Comment</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="k in keys" :key="k.id" :class="{ 'row-selected': k.selected }">
              <td><input type="checkbox" v-model="k.selected" /></td>
              <td><code>{{ k.algorithm }}</code></td>
              <td><code class="fp">{{ k.fingerprint }}</code></td>
              <td>{{ k.name ?? '—' }}</td>
              <td class="td-sec">{{ k.comment ?? '—' }}</td>
            </tr>
          </tbody>
        </table>

        <div class="action-bar">
          <button
            class="btn-danger"
            :disabled="selected.length === 0"
            @click="confirming = true"
          >
            Delete {{ selected.length }} key{{ selected.length !== 1 ? 's' : '' }}
          </button>
        </div>
      </template>
    </template>

    <!-- Confirm step -->
    <div v-if="confirming" class="modal-overlay" @click.self="confirming = false">
      <div class="modal">
        <h2>Confirm deletion</h2>
        <p>Delete {{ selected.length }} SSH key{{ selected.length !== 1 ? 's' : '' }}? This cannot be undone.</p>
        <div class="modal-actions">
          <button class="btn-secondary" @click="confirming = false">Cancel</button>
          <button class="btn-danger" :disabled="purging" @click="handlePurge">
            {{ purging ? 'Deleting…' : 'Delete' }}
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header { margin-bottom: 1.5rem; h1 { margin: 0.25rem 0 0; } }
.back-link { color: #94a3b8; text-decoration: none; font-size: 0.875rem; &:hover { color: #e2e8f0; } }

.toolbar {
  display: flex; align-items: center; gap: 1rem; margin-bottom: 0.75rem;
}

.count { font-size: 0.875rem; color: #64748b; }

.checkbox-label {
  display: flex; align-items: center; gap: 0.375rem; font-size: 0.875rem; color: #94a3b8;
  cursor: pointer;
}

.data-table {
  width: 100%; border-collapse: collapse; font-size: 0.875rem;
  .th-check { width: 32px; }
  th {
    text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid #2d3248;
    color: #64748b; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em;
  }
  td { padding: 0.5rem 0.75rem; border-bottom: 1px solid #1e2235; color: #e2e8f0; }
  code { background: #0f1117; padding: 0.1em 0.35em; border-radius: 3px; }
  .fp { font-size: 0.75rem; word-break: break-all; }
  .row-selected td { background: rgba(239,68,68,.05); }
}

.td-sec { color: #64748b; }

.action-bar { margin-top: 1rem; }

.modal-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,.6);
  display: flex; align-items: center; justify-content: center; z-index: 100;
}

.modal {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px;
  padding: 2rem; width: 100%; max-width: 400px;
  h2 { margin: 0 0 1rem; }
  p { color: #94a3b8; margin-bottom: 1.5rem; }
}

.modal-actions { display: flex; gap: 0.75rem; justify-content: flex-end; }

.btn-danger {
  padding: 0.5rem 1.25rem; background: rgba(239,68,68,.15); border: 1px solid #ef4444;
  border-radius: 4px; color: #fca5a5; font-size: 0.9375rem; cursor: pointer;
  &:hover:not(:disabled) { background: rgba(239,68,68,.25); }
  &:disabled { opacity: 0.6; cursor: not-allowed; }
}

.btn-secondary {
  padding: 0.5rem 1.25rem; background: none; border: 1px solid #2d3248; border-radius: 4px;
  color: #94a3b8; font-size: 0.9375rem; cursor: pointer;
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
}

.loading { color: #94a3b8; }
.error-msg { color: #fca5a5; }
.empty { color: #64748b; }
</style>
