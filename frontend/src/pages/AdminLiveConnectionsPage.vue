<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { storeToRefs } from 'pinia'
import AppShell from '@/components/AppShell.vue'
import StatusDot from '@/components/StatusDot.vue'
import { roleBadgeLabel } from '@/labels'
import type { RemoteStatus } from '@/liveStatus'
import { useLiveConnectionsStore } from '@/stores/liveConnections'

interface AdminRow {
  live: boolean
  remote_status: RemoteStatus | null
  self_status: RemoteStatus | null
  username: string
  entity_id: string
  entity_name: string | null
  role: 'server' | 'client'
  service_name: string
  port: number
  peer_ip: string | null
}

const liveConnections = useLiveConnectionsStore()
const { snapshots, connected } = storeToRefs(liveConnections)

// This page is the one place the shared connection switches to the
// unfiltered, admin-only "all" scope — reverted back to "mine" on leaving so
// toasts/other pages don't keep seeing everyone else's activity.
onMounted(() => liveConnections.setScope('all'))
onUnmounted(() => liveConnections.setScope('mine'))

const loading = computed(() => !connected.value)

// Flattened client-side from the shared store's nested-per-entity snapshots
// — mirrors the former server-flattened `LiveConnectionRow[]`.
const rows = computed((): AdminRow[] => {
  const out: AdminRow[] = []
  for (const snap of snapshots.value) {
    const username = snap.account?.username ?? '—'
    for (const service of snap.services) {
      out.push({
        live: service.live,
        remote_status: service.subscribers.length > 0 ? 'active' : 'offline',
        self_status: service.remote_status,
        username,
        entity_id: snap.entity_id,
        entity_name: snap.entity_name,
        role: 'server',
        service_name: service.service_name,
        port: service.proxy_port,
        peer_ip: null,
      })
    }
    for (const sub of snap.subscriptions) {
      out.push({
        live: sub.live,
        remote_status: sub.remote_status,
        self_status: null,
        username,
        entity_id: snap.entity_id,
        entity_name: snap.entity_name,
        role: 'client',
        service_name: sub.service_name,
        port: sub.subscriber_local_port,
        peer_ip: sub.peer_ip,
      })
    }
  }
  return out
})

const filterUser = ref('')
const filterRole = ref<'' | 'server' | 'client'>('')
const filterService = ref('')

const usernames = computed(() => Array.from(new Set(rows.value.map(r => r.username))).sort())
const serviceNames = computed(() => Array.from(new Set(rows.value.map(r => r.service_name))).sort())

// The Peer IP column is genuinely useful data (client rows carry a real address) but
// server-role rows never have one — if literally no row in the current fetch has a
// peer IP, drop the whole column rather than render an all-"—" column.
const showPeerIp = computed(() => rows.value.some(r => r.peer_ip))

// Once filtered down to one exact account, the Account column is redundant —
// every visible row already shares it.
const showAccount = computed(() => !filterUser.value)

const filteredRows = computed(() =>
  rows.value.filter(r =>
    (!filterUser.value || r.username === filterUser.value)
    && (!filterRole.value || r.role === filterRole.value)
    && (!filterService.value || r.service_name === filterService.value),
  ),
)
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
            <th v-if="showAccount">Account</th>
            <th>Entity</th>
            <th>Service</th>
            <th>Port</th>
            <th v-if="showPeerIp">Peer IP</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, idx) in filteredRows" :key="idx">
            <td><StatusDot :live="row.live" :self-status="row.self_status" :remote-status="row.remote_status" /></td>
            <td v-if="showAccount">{{ row.username }}</td>
            <td>
              <RouterLink :to="{ name: 'entity-detail', params: { id: row.entity_id } }">
                {{ row.entity_name ?? row.entity_id.slice(0, 13) + '…' }}
              </RouterLink>
              <span class="role-tag">{{ roleBadgeLabel[row.role] }}</span>
            </td>
            <td>{{ row.service_name }}</td>
            <td><a :href="`http://localhost:${row.port}`" class="port-link">{{ row.port }}</a></td>
            <td v-if="showPeerIp">{{ row.peer_ip ?? '—' }}</td>
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

.role-tag {
  margin-left: 0.5rem;
  font-size: 0.75rem;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.port-link { font-family: monospace; }

.empty { color: #64748b; }
.loading { color: #94a3b8; }
</style>
