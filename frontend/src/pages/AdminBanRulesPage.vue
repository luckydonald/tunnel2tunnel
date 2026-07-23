<script setup lang="ts">
import { ref, onMounted } from 'vue'
import AppShell from '@/components/AppShell.vue'
import { adminApi, type BanRule, type TarpitAction, type TarpitSettings, type TarpitThreshold } from '@/api/admin'
import {
  banScopeTypeLabel,
  ruleScopeTypeOptions,
  tarpitActionLabel,
  tarpitActionOptions,
  type RuleScopeType,
} from '@/labels'
import { useToast } from '@/composables/useToast'

const { show: toast } = useToast()

const rules = ref<BanRule[]>([])
const rulesLoading = ref(true)

async function loadRules(): Promise<void> {
  rulesLoading.value = true
  try {
    rules.value = await adminApi.listBanRules()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load ban rules')
  } finally {
    rulesLoading.value = false
  }
}

const showAdd = ref(false)
type NewRule = {
  scope_type: RuleScopeType
  peer_ip: string
  user_id: string
  reason: string
  active_until: string
  action: TarpitAction
  fail_count: number
  window_seconds: number
}
const blankRule = (): NewRule => ({
  scope_type: 'global',
  peer_ip: '',
  user_id: '',
  reason: '',
  active_until: '',
  action: 'trap',
  fail_count: 5,
  window_seconds: 600,
})
const newRule = ref<NewRule>(blankRule())
const adding = ref(false)

function canAdd(r: NewRule): boolean {
  if (r.scope_type === 'global') return !!r.fail_count && !!r.window_seconds
  if (r.scope_type === 'peer_ip') return !!r.peer_ip
  return !!r.user_id
}

async function handleAdd(): Promise<void> {
  adding.value = true
  try {
    if (newRule.value.scope_type === 'global') {
      await adminApi.createTarpitThreshold({
        fail_count: newRule.value.fail_count,
        window_seconds: newRule.value.window_seconds,
        enabled: true,
        action: newRule.value.action,
      })
      await loadThresholds()
    } else {
      await adminApi.createBanRule({
        scope_type: newRule.value.scope_type,
        peer_ip: newRule.value.scope_type === 'peer_ip' ? newRule.value.peer_ip : null,
        user_id: newRule.value.scope_type === 'user' ? newRule.value.user_id : null,
        reason: newRule.value.reason || null,
        active_until: newRule.value.active_until ? new Date(newRule.value.active_until).toISOString() : null,
        action: newRule.value.action,
      })
      await loadRules()
    }
    showAdd.value = false
    newRule.value = blankRule()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to add rule')
  } finally {
    adding.value = false
  }
}

async function handleDelete(id: string): Promise<void> {
  if (!confirm('Remove this ban rule?')) return
  try {
    await adminApi.deleteBanRule(id)
    await loadRules()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to remove ban rule')
  }
}

async function handleRestore(id: string): Promise<void> {
  try {
    await adminApi.restoreBanRule(id)
    await loadRules()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to restore ban rule')
  }
}

function isExpired(rule: BanRule): boolean {
  return !!rule.active_until && new Date(rule.active_until).getTime() < Date.now()
}

// Global thresholds
const settings = ref<TarpitSettings | null>(null)
const savingSettings = ref(false)

async function loadSettings(): Promise<void> {
  try {
    settings.value = await adminApi.getTarpitSettings()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load tarpit settings')
  }
}

async function saveSettings(): Promise<void> {
  if (!settings.value) return
  savingSettings.value = true
  try {
    await adminApi.updateTarpitSettings(settings.value)
    toast('Settings saved')
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to save settings')
  } finally {
    savingSettings.value = false
  }
}

// Threshold rules (multiple independent count+window rules)
const thresholds = ref<TarpitThreshold[]>([])
const thresholdsLoading = ref(true)

async function loadThresholds(): Promise<void> {
  thresholdsLoading.value = true
  try {
    thresholds.value = await adminApi.listTarpitThresholds()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load threshold rules')
  } finally {
    thresholdsLoading.value = false
  }
}

async function handleUpdateThreshold(t: TarpitThreshold): Promise<void> {
  try {
    await adminApi.updateTarpitThreshold(t.id, {
      fail_count: t.fail_count,
      window_seconds: t.window_seconds,
      enabled: t.enabled,
      action: t.action,
    })
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to update threshold rule')
    await loadThresholds()
  }
}

async function handleDeleteThreshold(id: string): Promise<void> {
  if (!confirm('Remove this threshold rule?')) return
  try {
    await adminApi.deleteTarpitThreshold(id)
    await loadThresholds()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to remove threshold rule')
  }
}

async function handleRestoreThreshold(id: string): Promise<void> {
  try {
    await adminApi.restoreTarpitThreshold(id)
    await loadThresholds()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to restore threshold rule')
  }
}

function formatWindow(seconds: number): string {
  if (seconds % 86400 === 0) return `${seconds / 86400}d`
  if (seconds % 3600 === 0) return `${seconds / 3600}h`
  if (seconds % 60 === 0) return `${seconds / 60}m`
  return `${seconds}s`
}

onMounted(() => {
  loadRules()
  loadSettings()
  loadThresholds()
})
</script>

<template>
  <AppShell>
    <div class="page-header">
      <h1>Ban Rules</h1>
    </div>

    <section class="section">
      <h2>Tarpit Enforcement</h2>
      <div v-if="settings" class="thresholds-card">
        <label class="checkbox-label">
          <input v-model="settings.enabled" type="checkbox" /> Tarpit/ban enforcement enabled
        </label>
        <button class="btn-primary" :disabled="savingSettings" @click="saveSettings">
          {{ savingSettings ? 'Saving…' : 'Save' }}
        </button>
      </div>
    </section>

    <section class="section">
      <div class="section-header">
        <h2>Rules</h2>
        <button class="btn-secondary" @click="showAdd = !showAdd">{{ showAdd ? 'Cancel' : 'Add rule' }}</button>
      </div>

      <div v-if="showAdd" class="add-form">
        <select v-model="newRule.scope_type" class="select-sm">
          <option v-for="opt in ruleScopeTypeOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>

        <template v-if="newRule.scope_type === 'global'">
          <input v-model.number="newRule.fail_count" type="number" min="1" class="input-sm" placeholder="Failed attempts" />
          <span class="section-note">in</span>
          <input v-model.number="newRule.window_seconds" type="number" min="1" class="input-sm" placeholder="Window (seconds)" />
          <span class="section-note">seconds</span>
        </template>
        <template v-else>
          <input v-if="newRule.scope_type === 'peer_ip'" v-model="newRule.peer_ip" type="text" class="input-sm" placeholder="Peer IP" />
          <input v-else v-model="newRule.user_id" type="text" class="input-sm" placeholder="User ID (UUID)" />
          <input v-model="newRule.reason" type="text" class="input-sm" placeholder="Reason (optional)" />
          <input v-model="newRule.active_until" type="datetime-local" class="input-sm" title="Active until (blank = indefinite)" />
        </template>

        <select v-model="newRule.action" class="select-sm">
          <option v-for="opt in tarpitActionOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
        <button class="btn-primary" :disabled="adding || !canAdd(newRule)" @click="handleAdd">
          {{ adding ? 'Adding…' : 'Add' }}
        </button>
      </div>

      <h3 class="subsection-title">Global thresholds</h3>
      <div v-if="thresholdsLoading" class="loading">Loading…</div>
      <table v-else-if="thresholds.length" class="data-table">
        <thead>
          <tr>
            <th>Failed attempts</th>
            <th>Window</th>
            <th>Action</th>
            <th>Enabled</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="t in thresholds" :key="t.id" :id="`threshold-${t.id}`" :class="{ 'row-deleted': t.deleted_at }">
            <td>{{ t.fail_count }}</td>
            <td>{{ formatWindow(t.window_seconds) }}</td>
            <td>
              <select v-model="t.action" class="select-sm" :disabled="!!t.deleted_at" @change="handleUpdateThreshold(t)">
                <option v-for="opt in tarpitActionOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
              </select>
            </td>
            <td>
              <input type="checkbox" v-model="t.enabled" :disabled="!!t.deleted_at" @change="handleUpdateThreshold(t)" />
            </td>
            <td>
              <span v-if="t.deleted_at" class="deleted-tag">Deleted</span>
              <button v-if="t.deleted_at" class="btn-secondary btn-restore" @click="handleRestoreThreshold(t.id)">Restore</button>
              <button v-else class="btn-del-sm" @click="handleDeleteThreshold(t.id)">×</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">No threshold rules — tarpit/ban auto-trigger is inert until at least one is added.</p>

      <h3 class="subsection-title">IP / user rules</h3>
      <div v-if="rulesLoading" class="loading">Loading…</div>
      <table v-else-if="rules.length" class="data-table">
        <thead>
          <tr>
            <th>Scope</th>
            <th>Target</th>
            <th>Action</th>
            <th>Reason</th>
            <th>Active until</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in rules" :key="r.id" :id="`rule-${r.id}`" :class="{ 'row-expired': isExpired(r), 'row-deleted': r.deleted_at }">
            <td>{{ banScopeTypeLabel[r.scope_type] }}</td>
            <td><code class="fp">{{ r.peer_ip ?? r.user_id }}</code></td>
            <td>{{ tarpitActionLabel[r.action] }}</td>
            <td class="td-desc">{{ r.reason ?? '—' }}</td>
            <td class="td-ts">
              {{ r.active_until ? new Date(r.active_until).toLocaleString() : 'Indefinite' }}
              <span v-if="isExpired(r)" class="expired-tag">(expired)</span>
            </td>
            <td>
              <span v-if="r.deleted_at" class="deleted-tag">Deleted</span>
              <button v-if="r.deleted_at" class="btn-secondary btn-restore" @click="handleRestore(r.id)">Restore</button>
              <button v-else class="btn-del-sm" @click="handleDelete(r.id)">×</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">No ban rules.</p>
    </section>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; h1 { margin: 0; } }

.section {
  margin-bottom: 2.5rem;
  h2 { font-size: 1rem; color: #94a3b8; margin-bottom: 1rem; text-transform: uppercase; letter-spacing: 0.06em; }
}
.section-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem; h2 { margin: 0; } }
.subsection-title { font-size: 0.8125rem; color: #64748b; margin: 1.25rem 0 0.5rem; text-transform: uppercase; letter-spacing: 0.05em; }

.thresholds-card {
  display: flex; align-items: center; gap: 1.25rem; flex-wrap: wrap;
  padding: 1rem; background: #1a1d27; border-radius: 6px;
}
.checkbox-label { display: flex; align-items: center; gap: 0.375rem; color: #94a3b8; font-size: 0.875rem; }
.section-note { color: #64748b; font-size: 0.875rem; }
.field-inline {
  display: flex; align-items: center; gap: 0.5rem; color: #94a3b8; font-size: 0.875rem;
  label { white-space: nowrap; }
}

.add-form {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
  padding: 0.75rem; background: #1a1d27; border-radius: 6px; margin-bottom: 0.75rem;
}

.input-sm, .select-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.875rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.loading { color: #94a3b8; }
.empty { color: #64748b; }

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

.row-expired { opacity: 0.55; }
.row-deleted { opacity: 0.55; }
.expired-tag { color: #64748b; }
.deleted-tag {
  color: #f87171; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.04em;
  margin-right: 0.5rem;
}
.btn-restore { padding: .125rem .5rem; font-size: .8125rem; }
.td-desc { color: #64748b; }
.td-ts { font-size: 0.8125rem; color: #94a3b8; white-space: nowrap; }

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
</style>
