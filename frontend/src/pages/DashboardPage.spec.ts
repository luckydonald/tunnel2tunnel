import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createRouter, createWebHistory } from 'vue-router'
import DashboardPage from './DashboardPage.vue'
import { useAuthStore } from '@/stores/auth'
import { useLiveConnectionsStore } from '@/stores/liveConnections'
import type { User } from '@/api/auth'
import type { EntityLiveSnapshot } from '@/api/liveConnections'

function baseSnapshot(overrides: Partial<EntityLiveSnapshot> = {}): EntityLiveSnapshot {
  return {
    entity_id: 'e1',
    entity_name: 'home-nas',
    entity_online: true,
    entity_last_disconnected_at: null,
    mine: true,
    services: [],
    subscriptions: [],
    ...overrides,
  }
}

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
  })

  it('flattens services (role: server) and subscriptions (role: client) from each owned entity', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = [
      baseSnapshot({
        services: [
          {
            port_config_id: 'p1', service_name: 'VNC', proxy_port: 5900, live: true, remote_status: null,
            subscribers: [{
              entity: { id: 'e2', name: null },
              account: { user_id: 'u2', username: 'friend' },
              peer_ip: '1.2.3.4',
              connected_since: '2026-01-01T00:00:00Z',
            }],
          },
        ],
        subscriptions: [
          {
            subscription_id: 's1', port_config_id: 'p2', owner: { id: 'e1', name: 'home-nas' },
            service_name: 'Postgres', proxy_port: 5555, subscriber_local_port: 5555, enabled: true,
            live: false, remote_status: 'not_forwarded', peer_ip: null, connected_since: null,
          },
        ],
      }),
    ]

    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('VNC')
    expect(wrapper.text()).toContain('Postgres')
    expect(wrapper.text()).toContain('🖧 Server')
    expect(wrapper.text()).toContain('💻 Client')
  })

  it('renders an independent ring for a subscription row whose remote is connected but hasn\'t forwarded the port yet', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = [
      baseSnapshot({
        subscriptions: [
          {
            subscription_id: 's1', port_config_id: 'p2', owner: { id: 'e1', name: 'home-nas' },
            service_name: 'Postgres', proxy_port: 5555, subscriber_local_port: 5555, enabled: true,
            live: false, remote_status: 'not_forwarded', peer_ip: null, connected_since: null,
          },
        ],
      }),
    ]

    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()

    const dot = wrapper.find('.status-dot')
    expect(dot.classes()).toContain('not-live')
    expect(dot.classes()).toContain('ring-not_forwarded')
  })

  it('shows an empty message when the user owns no services/subscriptions', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = []
    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('No services or subscriptions configured yet.')
  })

  it('only shows the "See all" admin link for admin users', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = []
    const wrapper = await mountDashboard()
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).not.toContain('See all')

    const auth = useAuthStore()
    auth.user = { id: 'u1', username: 'admin', email: null, is_admin: true, description: null } satisfies User
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('See all')
  })
})
