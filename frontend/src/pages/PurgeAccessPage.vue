<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import AppShell from '@/components/AppShell.vue'
import { adminApi } from '@/api/admin'
import { subjectTypeLabel } from '@/labels'
import type { AccessRule } from '@/api/friends'

interface RuleRow {
  id: string
  owner_entity_id: string
  owner_entity_name: string | null
  subject_type: AccessRule['subject_type']
  subject_entity_id: string | null
  subject_entity_name: string | null
  subject_user_id: string | null
  hostname: string | null
  port_config_id: string | null
  port_config_name: string | null
  selected: boolean
}

const rules = ref<RuleRow[]>([])
const loading = ref(true)
const error = ref<string | null>(null)
const purging = ref(false)
const confirming = ref(false)

onMounted(async () => {
  try {
    const raw = await adminApi.listMyAccess()
    rules.value = raw.map(r => ({ ...r, subject_type: r.subject_type as AccessRule['subject_type'], selected: false }))
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load'
  } finally {
    loading.value = false
  }
})

const selected = computed(() => rules.value.filter(r => r.selected))
const allSelected = computed(() => rules.value.length > 0 && rules.value.every(r => r.selected))

function toggleAll(): void {
  const next = !allSelected.value
  rules.value.forEach(r => (r.selected = next))
}

async function handlePurge(): Promise<void> {
  purging.value = true
  try {
    await adminApi.purgeAccess(selected.value.map(r => r.id))
    rules.value = rules.value.filter(r => !r.selected)
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
        <h1>Purge Access Rules</h1>
      </div>
    </div>

    <div v-if="loading" class="loading">Loading…</div>
    <div v-else-if="error" class="error-msg">{{ error }}</div>
    <template v-else>
      <p v-if="!rules.length" class="empty">No access rules found.</p>
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
              <th>Owner entity</th>
              <th>Subject</th>
              <th>Scope</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in rules" :key="r.id" :class="{ 'row-selected': r.selected }">
              <td><input type="checkbox" v-model="r.selected" /></td>
              <td>
                <RouterLink :to="{ name: 'entity-detail', params: { id: r.owner_entity_id } }">
                  {{ r.owner_entity_name ?? r.owner_entity_id.slice(0, 13) + '…' }}
                </RouterLink>
              </td>
              <td>
                {{ subjectTypeLabel[r.subject_type] }}
                <RouterLink
                  v-if="r.subject_entity_id"
                  :to="{ name: 'entity-detail', params: { id: r.subject_entity_id } }"
                  class="subject-detail"
                >{{ r.subject_entity_name ?? r.subject_entity_id.slice(0, 13) + '…' }}</RouterLink>
                <code v-else-if="r.subject_user_id" class="subject-detail uuid">{{ r.subject_user_id.slice(0, 13) }}…</code>
                <span v-if="r.hostname" class="hostname-hint">as “{{ r.hostname }}”</span>
              </td>
              <td>{{ r.port_config_id ? `Port: ${r.port_config_name ?? r.port_config_id.slice(0, 8) + '…'}` : 'Whole entity' }}</td>
            </tr>
          </tbody>
        </table>

        <div class="action-bar">
          <button
            class="btn-danger"
            :disabled="selected.length === 0"
            @click="confirming = true"
          >
            Delete {{ selected.length }} rule{{ selected.length !== 1 ? 's' : '' }}
          </button>
        </div>
      </template>
    </template>

    <!-- Confirm step -->
    <div v-if="confirming" class="modal-overlay" @click.self="confirming = false">
      <div class="modal">
        <h2>Confirm deletion</h2>
        <p>Delete {{ selected.length }} access rule{{ selected.length !== 1 ? 's' : '' }}? This cannot be undone.</p>
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

.toolbar { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.75rem; }
.count { font-size: 0.875rem; color: #64748b; }
.checkbox-label {
  display: flex; align-items: center; gap: 0.375rem; font-size: 0.875rem; color: #94a3b8; cursor: pointer;
}

.data-table {
  width: 100%; border-collapse: collapse; font-size: 0.875rem;
  .th-check { width: 32px; }
  th {
    text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid #2d3248;
    color: #64748b; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em;
  }
  td { padding: 0.5rem 0.75rem; border-bottom: 1px solid #1e2235; color: #e2e8f0; }
  code { background: #0f1117; padding: 0.1em 0.35em; border-radius: 3px; font-size: 0.875em; }
  .uuid { font-size: 0.75rem; word-break: break-all; }
  .row-selected td { background: rgba(239,68,68,.05); }
  a { color: #7dd3fc; text-decoration: none; &:hover { text-decoration: underline; } }
}

.subject-detail { margin-left: 0.375rem; font-size: 0.8125rem; }
.hostname-hint { margin-left: 0.375rem; font-size: 0.75rem; color: #64748b; }

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
