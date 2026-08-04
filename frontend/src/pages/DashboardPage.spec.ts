import { describe, it, expect, beforeEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createRouter, createWebHistory } from 'vue-router'
import DashboardPage from './DashboardPage.vue'
import { useAuthStore } from '@/stores/auth'
import type { User } from '@/api/auth'
import type { RemoteStatus } from '@/liveStatus'

interface DashboardRow {
  live: boolean
  remote_status: RemoteStatus | null
  entity_id: string
  entity_name: string | null
  role: 'server' | 'client'
  service_name: string
  port: number
  connected_since: string | null
}

let latestRows: DashboardRow[] = []

vi.mock('@/composables/useLiveSocket', () => ({
  useLiveSocket: (_path: string, onMessage: (data: DashboardRow[]) => void) => {
    onMessage(latestRows)
  },
}))

async function mountDashboard() {
  const router = createRouter({
    history: createWebHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/entities/:id', name: 'entity-detail', component: { template: '<div />' } },
      { path: '/admin/live-connections', name: 'admin-live-connections', component: { template: '<div />' } },
    ],
  })
  const wrapper = mount(DashboardPage, { global: { plugins: [router] } })
  await router.isReady()
  return wrapper
}

describe('DashboardPage', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    latestRows = []
  })

  it('flattens services (role: server) and subscriptions (role: client) from each owned entity', async () => {
    latestRows = [
      {
        live: true, remote_status: null, entity_id: 'e1', entity_name: 'home-nas',
        role: 'server', service_name: 'VNC', port: 5900, connected_since: '2026-01-01T00:00:00Z',
      },
      {
        live: false, remote_status: 'orange', entity_id: 'e1', entity_name: 'home-nas',
        role: 'client', service_name: 'Postgres', port: 5555, connected_since: null,
      },
    ]

    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('VNC')
    expect(wrapper.text()).toContain('Postgres')
    expect(wrapper.text()).toContain('🖧 Server')
    expect(wrapper.text()).toContain('💻 Client')
  })

  it('renders an independent ring for a subscription row whose remote is connected but not providing the port yet', async () => {
    latestRows = [
      {
        live: false, remote_status: 'orange', entity_id: 'e1', entity_name: 'home-nas',
        role: 'client', service_name: 'Postgres', port: 5555, connected_since: null,
      },
    ]

    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()

    const dot = wrapper.find('.status-dot')
    expect(dot.classes()).toContain('not-live')
    expect(dot.classes()).toContain('ring-orange')
  })

  it('shows an empty message when the user owns no services/subscriptions', async () => {
    latestRows = []
    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('No services or subscriptions configured yet.')
  })

  it('only shows the "See all" admin link for admin users', async () => {
    latestRows = []
    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).not.toContain('See all')

    const auth = useAuthStore()
    auth.user = { id: 'u1', username: 'admin', email: null, is_admin: true, description: null } satisfies User
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('See all')
  })
})
