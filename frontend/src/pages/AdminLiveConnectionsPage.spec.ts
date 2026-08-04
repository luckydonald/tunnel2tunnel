import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createRouter, createWebHistory } from 'vue-router'
import AdminLiveConnectionsPage from './AdminLiveConnectionsPage.vue'
import { adminApi, type LiveConnectionRow } from '@/api/admin'

vi.mock('@/api/admin', async () => {
  const actual = await vi.importActual<typeof import('@/api/admin')>('@/api/admin')
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listLiveConnections: vi.fn(),
    },
  }
})

function makeRow(overrides: Partial<LiveConnectionRow> = {}): LiveConnectionRow {
  return {
    live: true,
    remote_status: null,
    account: { user_id: 'u1', username: 'alice' },
    entity: { id: 'e1', name: 'home-nas' },
    role: 'server',
    service_name: 'VNC',
    port: 5900,
    peer_ip: null,
    connected_since: '2026-01-01T00:00:00Z',
    ...overrides,
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
    vi.mocked(adminApi.listLiveConnections).mockReset()
  })

  it('renders one row per leg', async () => {
    vi.mocked(adminApi.listLiveConnections).mockResolvedValue([
      makeRow({ role: 'server', service_name: 'VNC' }),
      makeRow({ role: 'client', service_name: 'Postgres', account: { user_id: 'u2', username: 'bob' } }),
    ])
    const wrapper = await mountPage()
    expect(wrapper.text()).toContain('alice')
    expect(wrapper.text()).toContain('bob')
    expect(wrapper.text()).toContain('VNC')
    expect(wrapper.text()).toContain('Postgres')
  })

  it('omits the Peer IP column entirely when no row has one', async () => {
    vi.mocked(adminApi.listLiveConnections).mockResolvedValue([makeRow({ peer_ip: null })])
    const wrapper = await mountPage()
    expect(wrapper.text()).not.toContain('Peer IP')
  })

  it('shows the Peer IP column when at least one row has one', async () => {
    vi.mocked(adminApi.listLiveConnections).mockResolvedValue([makeRow({ peer_ip: '1.2.3.4' })])
    const wrapper = await mountPage()
    expect(wrapper.text()).toContain('Peer IP')
    expect(wrapper.text()).toContain('1.2.3.4')
  })

  it('filters by role', async () => {
    vi.mocked(adminApi.listLiveConnections).mockResolvedValue([
      makeRow({ role: 'server', service_name: 'VNC' }),
      makeRow({ role: 'client', service_name: 'Postgres' }),
    ])
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
    vi.mocked(adminApi.listLiveConnections).mockResolvedValue([])
    const wrapper = await mountPage()
    expect(wrapper.text()).toContain('No live connections match.')
  })
})
