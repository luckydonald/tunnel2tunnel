<script setup lang="ts">
import { ref, onMounted } from 'vue'
import AppShell from '@/components/AppShell.vue'
import StatusDot from '@/components/StatusDot.vue'
import { entitiesApi, type LiveConnectionStatus } from '@/api/entities'
import { roleBadgeLabel } from '@/labels'
import { formatSince } from '@/liveStatus'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

export interface DashboardRow {
  status: LiveConnectionStatus
  entityId: string
  entityName: string | null
  role: 'server' | 'client'
  serviceName: string
  port: number
  connectedSince: string | null
}

const rows = ref<DashboardRow[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)

/**
 * There is no single "my live connections across all my entities" endpoint —
 * only a per-entity `GET .../live-connections`. So this fetches the user's own
 * entities, then calls that endpoint once per owned entity and flattens the
 * result. This is an N+1-per-page-load pattern; it's correct given the current
 * API surface, but a follow-up aggregate endpoint (e.g. `GET /api/me/live-connections`)
 * would be a reasonable optimization if the entity count per user grows large.
 */
async function load(): Promise<void> {
  loading.value = true
  loadError.value = null
  try {
    const entities = await entitiesApi.list()
    const perEntity = await Promise.all(
      entities.map(async e => {
        try {
          const live = await entitiesApi.getLiveConnections(e.id)
          const flattened: DashboardRow[] = []
          for (const service of live.services) {
            flattened.push({
              status: service.status,
              entityId: e.id,
              entityName: e.name,
              role: 'server',
              serviceName: service.service_name,
              port: service.proxy_port,
              connectedSince: service.subscribers[0]?.connected_since ?? null,
            })
          }
          for (const sub of live.subscriptions) {
            flattened.push({
              status: sub.status,
              entityId: e.id,
              entityName: e.name,
              role: 'client',
              serviceName: sub.service_name,
              port: sub.subscriber_local_port,
              connectedSince: sub.connected_since,
            })
          }
          return flattened
        } catch {
          // one entity's live-connections fetch failing shouldn't blank the whole dashboard
          return [] as DashboardRow[]
        }
      }),
    )
    rows.value = perEntity.flat()
  } catch (e) {
    loadError.value = e instanceof Error ? e.message : 'Failed to load live connections'
  } finally {
    loading.value = false
  }
}

onMounted(load)
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
      <div v-else-if="loadError" class="error-msg">{{ loadError }}</div>
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
              <td><StatusDot :status="row.status" /></td>
              <td>
                <RouterLink :to="{ name: 'entity-detail', params: { id: row.entityId } }">
                  {{ row.entityName ?? row.entityId.slice(0, 13) + '…' }}
                </RouterLink>
              </td>
              <td>{{ roleBadgeLabel[row.role] }}</td>
              <td>{{ row.serviceName }}</td>
              <td>{{ row.port }}</td>
              <td>{{ formatSince(row.connectedSince) }}</td>
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
.error-msg { color: #fca5a5; }
</style>
