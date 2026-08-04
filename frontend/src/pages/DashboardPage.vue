<script setup lang="ts">
import { ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import StatusDot from '@/components/StatusDot.vue'
import { roleBadgeLabel } from '@/labels'
import { formatSince, type RemoteStatus } from '@/liveStatus'
import { useAuthStore } from '@/stores/auth'
import { useLiveSocket } from '@/composables/useLiveSocket'

const auth = useAuthStore()

export interface DashboardRow {
  live: boolean
  remote_status: RemoteStatus | null
  entity_id: string
  entity_name: string | null
  role: 'server' | 'client'
  service_name: string
  port: number
  connected_since: string | null
}

const rows = ref<DashboardRow[]>([])
const loading = ref(true)

// Server-side aggregate over all of this user's own entities — replaces the
// previous N+1 client-side fetch (one `getLiveConnections` call per entity)
// with a single realtime WebSocket push.
useLiveSocket<DashboardRow[]>('/api/me/live-connections/ws', data => {
  rows.value = data
  loading.value = false
})
</script>

<template>
  <AppShell>
    <h1>Dashboard</h1>
    <p>Welcome, <strong>{{ auth.user?.username }}</strong>.</p>

    <section class="section">
      <div class="section-header">
        <h2>Your live connections</h2>
        <RouterLink v-if="auth.user?.is_admin" :to="{ name: 'admin-live-connections' }" class="see-all">See all →</RouterLink>
      </div>

      <div v-if="loading" class="loading">Loading…</div>
      <template v-else>
        <table v-if="rows.length" class="data-table">
          <thead>
            <tr>
              <th></th>
              <th>Entity</th>
              <th>Role</th>
              <th>Service</th>
              <th>Port</th>
              <th>Since</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, idx) in rows" :key="idx">
              <td><StatusDot :live="row.live" :remote-status="row.remote_status" /></td>
              <td>
                <RouterLink :to="{ name: 'entity-detail', params: { id: row.entity_id } }">
                  {{ row.entity_name ?? row.entity_id.slice(0, 13) + '…' }}
                </RouterLink>
              </td>
              <td>{{ roleBadgeLabel[row.role] }}</td>
              <td>{{ row.service_name }}</td>
              <td>{{ row.port }}</td>
              <td>{{ formatSince(row.connected_since) }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="empty">No services or subscriptions configured yet.</p>
      </template>
    </section>
  </AppShell>
</template>

<style lang="scss" scoped>
.section { margin-top: 2rem; }

.section-header {
  display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem;
  h2 { margin: 0; font-size: 1rem; color: #94a3b8; text-transform: uppercase; letter-spacing: 0.06em; }
}

.see-all { color: #7dd3fc; text-decoration: none; font-size: 0.875rem; &:hover { text-decoration: underline; } }

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
