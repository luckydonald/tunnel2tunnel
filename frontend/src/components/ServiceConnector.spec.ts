import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createRouter, createWebHistory } from 'vue-router'
import ServiceConnector from './ServiceConnector.vue'
import type { SubscribableOwner } from '@/api/entities'

function makeOwner(overrides: Partial<SubscribableOwner> = {}): SubscribableOwner {
  return {
    id: 'owner-1',
    name: 'home-nas',
    description: null,
    ip_whitelist: null,
    valid_until: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted_at: null,
    online: true,
    last_disconnected_at: null,
    is_server: true,
    is_client: false,
    services: [],
    ...overrides,
  }
}

async function mountConnector(owners: SubscribableOwner[], ownEntityIds: string[] = []) {
  const router = createRouter({
    history: createWebHistory(),
    routes: [{ path: '/', component: { template: '<div />' } }, { path: '/entities/:id', component: { template: '<div />' } }],
  })
  const wrapper = mount(ServiceConnector, {
    props: { subscribableOwners: owners, ownEntityIds },
    global: { plugins: [router] },
  })
  await router.isReady()
  return wrapper
}

describe('ServiceConnector filters', () => {
  it('defaults to "Configured" and shows nothing when there are no subscriptions', async () => {
    const owners = [makeOwner({
      services: [{
        id: 'svc-1', entity_id: 'owner-1', enabled: true, local_port: 5900, proxy_port: 5900,
        name: 'VNC', description: null, sort_order: 0, host: 'localhost',
        created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
        subscription: null,
      }],
    })]
    const wrapper = await mountConnector(owners)
    expect(wrapper.find('.owner-group').exists()).toBe(false)
    expect(wrapper.text()).toContain('No subscriptions yet')
  })

  it('"Configured" shows only services with a subscription', async () => {
    const owners = [makeOwner({
      services: [
        {
          id: 'svc-1', entity_id: 'owner-1', enabled: true, local_port: 5900, proxy_port: 5900,
          name: 'VNC', description: null, sort_order: 0, host: 'localhost',
          created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
          subscription: {
            id: 'sub-1', port_config_id: 'svc-1', subscriber_entity_id: 'me', subscriber_local_port: 5901,
            enabled: true, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
          },
        },
        {
          id: 'svc-2', entity_id: 'owner-1', enabled: true, local_port: 445, proxy_port: 445,
          name: 'Samba', description: null, sort_order: 0, host: 'localhost',
          created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
          subscription: null,
        },
      ],
    })]
    const wrapper = await mountConnector(owners)
    expect(wrapper.text()).toContain('VNC')
    expect(wrapper.text()).not.toContain('Samba')
  })

  it('does not show the Origin filter in "Configured" mode, but reveals it under "Unconfigured"/"All"', async () => {
    const owners = [makeOwner({ services: [] })]
    const wrapper = await mountConnector(owners)
    expect(wrapper.text()).not.toContain('Origin:')

    await wrapper.findAll('.filter-btn').find(b => b.text() === 'Unconfigured')!.trigger('click')
    expect(wrapper.text()).toContain('Origin:')
  })

  it('emits "subscribe" with the chosen local port when clicking Subscribe', async () => {
    const owners = [makeOwner({
      services: [{
        id: 'svc-1', entity_id: 'owner-1', enabled: true, local_port: 5900, proxy_port: 5900,
        name: 'VNC', description: null, sort_order: 0, host: 'localhost',
        created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
        subscription: null,
      }],
    })]
    const wrapper = await mountConnector(owners)
    await wrapper.findAll('.filter-btn').find(b => b.text() === 'All')!.trigger('click')

    await wrapper.get('.local-port-input').setValue(6000)
    await wrapper.get('.btn-primary').trigger('click')

    expect(wrapper.emitted('subscribe')).toEqual([['svc-1', 6000]])
  })

  it('emits "unsubscribe" when clicking Unsubscribe on a configured service', async () => {
    const owners = [makeOwner({
      services: [{
        id: 'svc-1', entity_id: 'owner-1', enabled: true, local_port: 5900, proxy_port: 5900,
        name: 'VNC', description: null, sort_order: 0, host: 'localhost',
        created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
        subscription: {
          id: 'sub-1', port_config_id: 'svc-1', subscriber_entity_id: 'me', subscriber_local_port: 5901,
          enabled: true, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
        },
      }],
    })]
    const wrapper = await mountConnector(owners)
    await wrapper.get('.btn-danger').trigger('click')
    expect(wrapper.emitted('unsubscribe')).toEqual([['sub-1']])
  })
})
