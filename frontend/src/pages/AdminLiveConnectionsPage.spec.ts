import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createRouter, createWebHistory } from 'vue-router'
import AdminLiveConnectionsPage from './AdminLiveConnectionsPage.vue'
import { useLiveConnectionsStore } from '@/stores/liveConnections'
import type { EntityLiveSnapshot } from '@/api/liveConnections'
import type { RemoteStatus } from '@/liveStatus'

interface RowSpec {
  username?: string
  entity_id?: string
  entity_name?: string | null
  role: 'server' | 'client'
  service_name: string
  port?: number
  peer_ip?: string | null
  connected_since?: string | null
  live?: boolean
  remote_status?: RemoteStatus | null
}

/** One snapshot carrying a single service (server-role) or subscription
 * (client-role) leg — mirrors the former one-row-per-leg `LiveConnectionRow`
 * fixture, now expressed as the nested shape the page flattens client-side. */
function makeSnapshot(spec: RowSpec): EntityLiveSnapshot {
  const {
    username = 'alice', entity_id = 'e1', entity_name = 'home-nas', role, service_name,
    port = 5900, peer_ip = null, connected_since = '2026-01-01T00:00:00Z', live = true, remote_status = null,
  } = spec
  return {
    entity_id,
    entity_name,
    entity_online: true,
    entity_last_disconnected_at: null,
    mine: false,
    account: { user_id: `${entity_id}-user`, username },
    services: role === 'server'
      ? [{ port_config_id: 'p1', service_name, proxy_port: port, live, remote_status, subscribers: [] }]
      : [],
    subscriptions: role === 'client'
      ? [{
          subscription_id: 's1', port_config_id: 'p2', owner: { id: 'owner', name: 'owner' },
          service_name, proxy_port: port, subscriber_local_port: port, enabled: true,
          live, remote_status, peer_ip, connected_since,
        }]
      : [],
  }
}

async function mountPage() {
  const router = createRouter({
    history: createWebHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/entities/:id', name: 'entity-detail', component: { template: '<div />' } },
    ],
  })
  const wrapper = mount(AdminLiveConnectionsPage, { global: { plugins: [router] } })
  await router.isReady()
  await new Promise(r => setTimeout(r, 0))
  await wrapper.vm.$nextTick()
  return wrapper
}

describe('AdminLiveConnectionsPage', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('renders one row per leg', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = [
      makeSnapshot({ role: 'server', service_name: 'VNC' }),
      makeSnapshot({ role: 'client', service_name: 'Postgres', username: 'bob', entity_id: 'e2' }),
    ]
    const wrapper = await mountPage()
    expect(wrapper.text()).toContain('alice')
    expect(wrapper.text()).toContain('bob')
    expect(wrapper.text()).toContain('VNC')
    expect(wrapper.text()).toContain('Postgres')
  })

  it('omits the Peer IP column entirely when no row has one', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = [makeSnapshot({ role: 'server', service_name: 'VNC' })]
    const wrapper = await mountPage()
    expect(wrapper.text()).not.toContain('Peer IP')
  })

  it('shows the Peer IP column when at least one row has one', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = [makeSnapshot({ role: 'client', service_name: 'Postgres', peer_ip: '1.2.3.4' })]
    const wrapper = await mountPage()
    expect(wrapper.text()).toContain('Peer IP')
    expect(wrapper.text()).toContain('1.2.3.4')
  })

  it('filters by role', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = [
      makeSnapshot({ role: 'server', service_name: 'VNC' }),
      makeSnapshot({ role: 'client', service_name: 'Postgres' }),
    ]
    const wrapper = await mountPage()
    const tableText = () => wrapper.find('tbody').text()
    expect(tableText()).toContain('VNC')
    expect(tableText()).toContain('Postgres')

    const roleSelect = wrapper.findAll('select')[1]
    await roleSelect.setValue('server')
    expect(tableText()).toContain('VNC')
    expect(tableText()).not.toContain('Postgres')
  })

  it('shows an empty message when there are no matching rows', async () => {
    const store = useLiveConnectionsStore()
    store.connected = true
    store.snapshots = []
    const wrapper = await mountPage()
    expect(wrapper.text()).toContain('No live connections match.')
  })
})
