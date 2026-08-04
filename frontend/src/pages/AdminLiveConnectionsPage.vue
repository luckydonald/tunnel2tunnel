<script setup lang="ts">
import { ref, computed } from 'vue'
import AppShell from '@/components/AppShell.vue'
import StatusDot from '@/components/StatusDot.vue'
import { type LiveConnectionRow } from '@/api/admin'
import { roleBadgeLabel } from '@/labels'
import { formatSince } from '@/liveStatus'
import { useLiveSocket } from '@/composables/useLiveSocket'

const rows = ref<LiveConnectionRow[]>([])
const loading = ref(true)

const filterUser = ref('')
const filterRole = ref<'' | 'server' | 'client'>('')
const filterService = ref('')

const usernames = computed(() => Array.from(new Set(rows.value.map(r => r.account.username))).sort())
const serviceNames = computed(() => Array.from(new Set(rows.value.map(r => r.service_name))).sort())

// The Peer IP column is genuinely useful data (client rows carry a real address) but
// server-role rows never have one — if literally no row in the current fetch has a
// peer IP, drop the whole column rather than render an all-"—" column.
const showPeerIp = computed(() => rows.value.some(r => r.peer_ip))

const filteredRows = computed(() =>
  rows.value.filter(r =>
    (!filterUser.value || r.account.username === filterUser.value)
    && (!filterRole.value || r.role === filterRole.value)
    && (!filterService.value || r.service_name === filterService.value),
  ),
)

useLiveSocket<LiveConnectionRow[]>('/api/admin/live-connections/ws', data => {
  rows.value = data
  loading.value = false
})
</script>

<template>
  <AppShell>
    <div class="page-header">
      <h1>Live Connections</h1>
    </div>

    <div class="filter-bar">
      <select v-model="filterUser" class="select-sm">
        <option value="">Any user</option>
        <option v-for="u in usernames" :key="u" :value="u">{{ u }}</option>
      </select>
      <select v-model="filterRole" class="select-sm">
        <option value="">Any role</option>
        <option value="server">{{ roleBadgeLabel.server }}</option>
        <option value="client">{{ roleBadgeLabel.client }}</option>
      </select>
      <select v-model="filterService" class="select-sm">
        <option value="">Any service</option>
        <option v-for="s in serviceNames" :key="s" :value="s">{{ s }}</option>
      </select>
    </div>

    <div v-if="loading" class="loading">Loading…</div>
    <template v-else>
      <table v-if="filteredRows.length" class="data-table">
        <thead>
          <tr>
            <th></th>
            <th>Account</th>
            <th>Entity</th>
            <th>Role</th>
            <th>Service</th>
            <th>Port</th>
            <th v-if="showPeerIp">Peer IP</th>
            <th>Since</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, idx) in filteredRows" :key="idx">
            <td><StatusDot :live="row.live" :remote-status="row.remote_status" /></td>
            <td>{{ row.account.username }}</td>
            <td>
              <RouterLink :to="{ name: 'entity-detail', params: { id: row.entity.id } }">
                {{ row.entity.name ?? row.entity.id.slice(0, 13) + '…' }}
              </RouterLink>
            </td>
            <td>{{ roleBadgeLabel[row.role] }}</td>
            <td>{{ row.service_name }}</td>
            <td>{{ row.port }}</td>
            <td v-if="showPeerIp">{{ row.peer_ip ?? '—' }}</td>
            <td>{{ formatSince(row.connected_since) }}</td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">No live connections match.</p>
    </template>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; h1 { margin: 0; } }

.filter-bar {
  display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;
  margin-bottom: 1.25rem; padding: 0.75rem; background: #1a1d27; border-radius: 6px;
}

.select-sm {
  padding: 0.25rem 0.5rem; background: #0f1117; border: 1px solid #2d3248;
  border-radius: 4px; color: #e2e8f0; font-size: 0.875rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.data-table {
  width: 100%; border-collapse: collapse; font-size: 0.9375rem;
  th {
    text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid #2d3248;
    color: #64748b; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em;
  }
  td { padding: 0.625rem 0.75rem; border-bottom: 1px solid #1e2235; color: #e2e8f0; }
  a { color: #7dd3fc; text-decoration: none; &:hover { text-decoration: underline; } }
}

.empty { color: #64748b; }
.loading { color: #94a3b8; }
</style>
