<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { RouterLink } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import { adminApi, type ConnLog, type CreateBanRuleParams } from '@/api/admin'
import { failReasonLabel, tarpitMethodLabel, tarpitMethodOptions } from '@/labels'
import { useToast } from '@/composables/useToast'

const { show: toast } = useToast()

const logs = ref<ConnLog[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 50
const loading = ref(false)

const filterPeerIp = ref('')
const filterUserId = ref('')
const filterSuccess = ref<'' | 'true' | 'false'>('')
const filterMethod = ref<'' | ConnLog['tarpit_method']>('')
const filterQ = ref('')

async function search(): Promise<void> {
  loading.value = true
  try {
    const result = await adminApi.searchConnectionLogs({
      page: page.value,
      page_size: pageSize,
      peer_ip: filterPeerIp.value || undefined,
      user_id: filterUserId.value || undefined,
      success: filterSuccess.value === '' ? undefined : filterSuccess.value === 'true',
      method: filterMethod.value || undefined,
      q: filterQ.value || undefined,
    })
    logs.value = result.items
    total.value = result.total
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load connection logs')
  } finally {
    loading.value = false
  }
}

function applyFilters(): void {
  page.value = 1
  search()
}

function nextPage(): void {
  if (page.value * pageSize >= total.value) return
  page.value += 1
  search()
}

function prevPage(): void {
  if (page.value <= 1) return
  page.value -= 1
  search()
}

// Quick-ban form, opened from a log row
const banningLog = ref<ConnLog | null>(null)
const banReason = ref('')
const banActiveUntil = ref('')
const banning = ref(false)

function openBanForm(log: ConnLog): void {
  banningLog.value = log
  banReason.value = ''
  banActiveUntil.value = ''
}

async function confirmBan(): Promise<void> {
  if (!banningLog.value) return
  const log = banningLog.value
  const params: CreateBanRuleParams = log.user_id
    ? { scope_type: 'user', user_id: log.user_id, reason: banReason.value || null }
    : { scope_type: 'peer_ip', peer_ip: log.peer_ip ?? undefined, reason: banReason.value || null }
  if (banActiveUntil.value) {
    params.active_until = new Date(banActiveUntil.value).toISOString()
  }
  banning.value = true
  try {
    await adminApi.createBanRule(params)
    banningLog.value = null
    toast('Ban rule created')
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to create ban rule')
  } finally {
    banning.value = false
  }
}

onMounted(search)
</script>

<template>
  <AppShell>
    <div class="page-header">
      <h1>Connection Logs</h1>
    </div>

    <div class="filter-bar">
      <input v-model="filterPeerIp" type="text" class="input-sm" placeholder="Peer IP" @keyup.enter="applyFilters" />
      <input v-model="filterUserId" type="text" class="input-sm" placeholder="User ID" @keyup.enter="applyFilters" />
      <select v-model="filterSuccess" class="select-sm">
        <option value="">Any result</option>
        <option value="true">Success</option>
        <option value="false">Failure</option>
      </select>
      <select v-model="filterMethod" class="select-sm">
        <option value="">Any tarpit method</option>
        <option v-for="opt in tarpitMethodOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
      </select>
      <input v-model="filterQ" type="text" class="input-sm" placeholder="Search…" @keyup.enter="applyFilters" />
      <button class="btn-primary" @click="applyFilters">Search</button>
    </div>

    <div v-if="loading" class="loading">Loading…</div>
    <template v-else>
      <table v-if="logs.length" class="data-table">
        <thead>
          <tr>
            <th>Started</th>
            <th>Ended</th>
            <th>Peer IP</th>
            <th>User</th>
            <th>Fingerprint</th>
            <th>Password</th>
            <th>Result</th>
            <th>Reason</th>
            <th>Tarpit</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="l in logs" :key="l.id">
            <td class="td-ts">{{ new Date(l.started_at).toLocaleString() }}</td>
            <td class="td-ts">{{ l.ended_at ? new Date(l.ended_at).toLocaleString() : '—' }}</td>
            <td>{{ l.peer_ip ?? '—' }}</td>
            <td>
              <RouterLink v-if="l.user_id" :to="{ path: '/admin/users', hash: `#user-${l.user_id}` }" :title="l.user_id">
                {{ l.attempted_username ?? l.user_id.slice(0, 8) + '…' }}
              </RouterLink>
              <span v-else>{{ l.attempted_username ?? '—' }}</span>
            </td>
            <td><code v-if="l.key_fingerprint" class="fp">{{ l.key_fingerprint }}</code><span v-else>—</span></td>
            <td class="td-desc">{{ l.attempted_password ?? '—' }}</td>
            <td>
              <span :class="['badge-result', l.success ? 'ok' : 'fail']">{{ l.success ? 'ok' : 'fail' }}</span>
            </td>
            <td class="td-desc">
              {{ (l.fail_reason && failReasonLabel[l.fail_reason]) ?? l.fail_reason ?? l.success_reason ?? '—' }}
            </td>
            <td class="td-desc">{{ l.tarpit_method ? tarpitMethodLabel[l.tarpit_method] : '—' }}</td>
            <td>
              <button v-if="!l.success" class="btn-secondary btn-ban" @click="openBanForm(l)">Ban</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">No connection logs match.</p>

      <div class="pager">
        <button class="btn-secondary" :disabled="page <= 1" @click="prevPage">Prev</button>
        <span>Page {{ page }} of {{ Math.max(1, Math.ceil(total / pageSize)) }} ({{ total }} total)</span>
        <button class="btn-secondary" :disabled="page * pageSize >= total" @click="nextPage">Next</button>
      </div>
    </template>

    <!-- Quick ban modal -->
    <div v-if="banningLog" class="modal-overlay" @click.self="banningLog = null">
      <div class="modal">
        <h2>Ban {{ banningLog.user_id ? 'user' : 'IP' }}</h2>
        <p class="section-note">
          {{ banningLog.user_id ? banningLog.user_id : banningLog.peer_ip }}
        </p>
        <div class="field">
          <label>Reason <span class="optional">(optional)</span></label>
          <input v-model="banReason" type="text" />
        </div>
        <div class="field">
          <label>Active until <span class="optional">(optional — blank = indefinite)</span></label>
          <input v-model="banActiveUntil" type="datetime-local" />
        </div>
        <div class="modal-actions">
          <button class="btn-secondary" @click="banningLog = null">Cancel</button>
          <button class="btn-primary" :disabled="banning" @click="confirmBan">
            {{ banning ? 'Banning…' : 'Ban' }}
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; h1 { margin: 0; } }

.filter-bar {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
  margin-bottom: 1.25rem; padding: 0.75rem; background: #1a1d27; border-radius: 6px;
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

.td-desc { color: #64748b; }
.td-ts { font-size: 0.8125rem; color: #94a3b8; white-space: nowrap; }

.badge-result {
  display: inline-block; padding: 0.1em 0.45em; border-radius: 4px;
  font-size: 0.75rem; font-weight: 600; text-transform: uppercase;
  &.ok   { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.fail { background: rgba(239,68,68,.15);  color: #fca5a5; }
}

.btn-ban {
  font-size: 0.75rem; padding: 0.2rem 0.6rem;
}

.pager {
  display: flex; align-items: center; gap: 1rem; margin-top: 1rem; color: #94a3b8; font-size: 0.875rem;
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
  &:hover:not(:disabled) { color: #e2e8f0; border-color: #4f6ef7; }
  &:disabled { opacity: .5; cursor: not-allowed; }
}

.modal-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,.6);
  display: flex; align-items: center; justify-content: center; z-index: 100;
}
.modal {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px;
  padding: 2rem; width: 100%; max-width: 440px;
  h2 { margin: 0 0 0.5rem; font-size: 1.125rem; }
}
.section-note { font-size: 0.8125rem; color: #64748b; margin: 0 0 1rem; word-break: break-all; }
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
