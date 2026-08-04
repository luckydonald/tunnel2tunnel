import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createRouter, createWebHistory } from 'vue-router'
import DashboardPage from './DashboardPage.vue'
import { entitiesApi, type Entity, type EntityLiveConnectionsResponse } from '@/api/entities'
import { useAuthStore } from '@/stores/auth'
import type { User } from '@/api/auth'

vi.mock('@/api/entities', async () => {
  const actual = await vi.importActual<typeof import('@/api/entities')>('@/api/entities')
  return {
    ...actual,
    entitiesApi: {
      ...actual.entitiesApi,
      list: vi.fn(),
      getLiveConnections: vi.fn(),
    },
  }
})

function makeEntity(overrides: Partial<Entity> = {}): Entity {
  return {
    id: 'e1', name: 'home-nas', description: null, ip_whitelist: null, valid_until: null,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', deleted_at: null,
    online: true, last_disconnected_at: null, is_server: true, is_client: false,
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
    vi.mocked(entitiesApi.list).mockReset()
    vi.mocked(entitiesApi.getLiveConnections).mockReset()
  })

  it('flattens services (role: server) and subscriptions (role: client) from each owned entity', async () => {
    vi.mocked(entitiesApi.list).mockResolvedValue([makeEntity()])
    const live: EntityLiveConnectionsResponse = {
      services: [{
        port_config_id: 'pc1', service_name: 'VNC', proxy_port: 5900, live: true, remote_status: null,
        subscribers: [{
          entity: { id: 'client1', name: 'my-laptop' },
          account: { user_id: 'u1', username: 'alice' },
          peer_ip: '1.2.3.4',
          connected_since: '2026-01-01T00:00:00Z',
        }],
      }],
      subscriptions: [{
        subscription_id: 'sub1', port_config_id: 'pc2', owner: { id: 'owner1', name: 'old-vps' },
        service_name: 'Postgres', proxy_port: 5432, subscriber_local_port: 5555, enabled: true,
        live: false, remote_status: 'orange', connected_since: null,
      }],
    }
    vi.mocked(entitiesApi.getLiveConnections).mockResolvedValue(live)

    const wrapper = await mountDashboard()
    await new Promise(r => setTimeout(r, 0))
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('VNC')
    expect(wrapper.text()).toContain('Postgres')
    expect(wrapper.text()).toContain('🖧 Server')
    expect(wrapper.text()).toContain('💻 Client')
  })

  it('renders an independent ring for a subscription row whose remote is connected but not providing the port yet', async () => {
    vi.mocked(entitiesApi.list).mockResolvedValue([makeEntity()])
    const live: EntityLiveConnectionsResponse = {
      services: [],
      subscriptions: [{
        subscription_id: 'sub1', port_config_id: 'pc2', owner: { id: 'owner1', name: 'old-vps' },
        service_name: 'Postgres', proxy_port: 5432, subscriber_local_port: 5555, enabled: true,
        live: false, remote_status: 'orange', connected_since: null,
      }],
    }
    vi.mocked(entitiesApi.getLiveConnections).mockResolvedValue(live)

    const wrapper = await mountDashboard()
    await new Promise(r => setTimeout(r, 0))
    await wrapper.vm.$nextTick()

    const dot = wrapper.find('.status-dot')
    expect(dot.classes()).toContain('not-live')
    expect(dot.classes()).toContain('ring-orange')
  })

  it('shows an empty message when the user owns no services/subscriptions', async () => {
    vi.mocked(entitiesApi.list).mockResolvedValue([])
    const wrapper = await mountDashboard()
    await new Promise(r => setTimeout(r, 0))
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('No services or subscriptions configured yet.')
  })

  it('only shows the "See all" admin link for admin users', async () => {
    vi.mocked(entitiesApi.list).mockResolvedValue([])
    const wrapper = await mountDashboard()
    await new Promise(r => setTimeout(r, 0))
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).not.toContain('See all')

    const auth = useAuthStore()
    auth.user = { id: 'u1', username: 'admin', email: null, is_admin: true, description: null } satisfies User
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('See all')
  })
})
