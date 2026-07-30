import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createRouter, createWebHistory } from 'vue-router'
import SshCommandDisplay from './SshCommandDisplay.vue'
import type { Entity, EntityPort } from '@/api/entities'

const entity: Entity = {
  id: 'entity-uuid-1234',
  entity_type: 'client',
  name: 'test-client',
  description: null,
  ip_whitelist: null,
  valid_until: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  deleted_at: null,
  online: false,
  last_disconnected_at: null,
}

const ports: EntityPort[] = []

async function mountDisplay() {
  const router = createRouter({
    history: createWebHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/settings', component: { template: '<div />' } },
      { path: '/entities/:id', component: { template: '<div />' } },
    ],
  })
  const wrapper = mount(SshCommandDisplay, {
    props: {
      entity,
      ports,
      filename: 'test_key',
      t2tHost: 'example.com',
      t2tSshPort: 2222,
    },
    global: {
      plugins: [router],
    },
  })
  await router.isReady()
  return wrapper
}

// The four fields linked between the command line and the manual-config block,
// with the values each one renders as and how to click into the manual-config row.
const fields = [
  { field: 'user', value: entity.id, manualLabel: 'User' },
  { field: 'host', value: 'example.com', manualLabel: 'Host' },
  { field: 'port', value: '2222', manualLabel: 'Port' },
  { field: 'key', value: '~/.ssh/test_key', manualLabel: 'Private key' },
]

function manualRowFor(wrapper: Awaited<ReturnType<typeof mountDisplay>>, manualLabel: string) {
  const row = wrapper
    .findAll('.manual-field-row')
    .find(r => r.find('.manual-field-label').text() === manualLabel)
  if (!row) throw new Error(`manual field row not found for label ${manualLabel}`)
  return row
}

function cmdFlagFor(wrapper: Awaited<ReturnType<typeof mountDisplay>>, value: string) {
  // The port field is rendered as a RouterLink with class `port-link`, not `cmd-flag`.
  const flag = wrapper
    .get('.cmd-text')
    .findAll('.cmd-flag, .port-link')
    .find(el => el.text() === value)
  if (!flag) throw new Error(`cmd-flag not found for value ${value}`)
  return flag
}

describe('SshCommandDisplay hover-linking', () => {
  for (const { field, value, manualLabel } of fields) {
    it(`command line -> manual config: hovering ${field} in the ssh command highlights the manual row`, async () => {
      const wrapper = await mountDisplay()
      const cmdFlag = cmdFlagFor(wrapper, value)
      const manualCode = manualRowFor(wrapper, manualLabel).find('.manual-field-code')

      expect(manualCode.classes()).not.toContain('is-hovered')

      await cmdFlag.trigger('mouseenter')
      expect(manualCode.classes()).toContain('is-hovered')

      await cmdFlag.trigger('mouseleave')
      expect(manualCode.classes()).not.toContain('is-hovered')
    })

    it(`manual config -> command line: hovering the manual ${manualLabel} row highlights the ssh command`, async () => {
      const wrapper = await mountDisplay()
      const manualCode = manualRowFor(wrapper, manualLabel).find('.manual-field-code')
      const cmdFlag = cmdFlagFor(wrapper, value)

      expect(cmdFlag.classes()).not.toContain('is-hovered')

      await manualCode.trigger('mouseenter')
      expect(cmdFlag.classes()).toContain('is-hovered')

      await manualCode.trigger('mouseleave')
      expect(cmdFlag.classes()).not.toContain('is-hovered')
    })
  }

  it('only highlights the hovered field, not the others', async () => {
    const wrapper = await mountDisplay()
    const userManualCode = manualRowFor(wrapper, 'User').find('.manual-field-code')
    const hostManualCode = manualRowFor(wrapper, 'Host').find('.manual-field-code')

    await cmdFlagFor(wrapper, entity.id).trigger('mouseenter')

    expect(userManualCode.classes()).toContain('is-hovered')
    expect(hostManualCode.classes()).not.toContain('is-hovered')
  })
})

describe('SshCommandDisplay copy buttons', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    })
  })

  for (const { value, manualLabel } of fields) {
    it(`copies the correct value for ${manualLabel}`, async () => {
      const wrapper = await mountDisplay()
      const copyButton = manualRowFor(wrapper, manualLabel).get('.btn-copy')

      await copyButton.trigger('click')

      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(value)
    })
  }

  it('shows "Copied!" after clicking, then reverts to "Copy" after the timeout', async () => {
    vi.useFakeTimers()
    try {
      const wrapper = await mountDisplay()
      const copyButton = manualRowFor(wrapper, 'User').get('.btn-copy')

      expect(copyButton.text()).toBe('Copy')

      await copyButton.trigger('click')
      await vi.advanceTimersByTimeAsync(0)
      expect(copyButton.text()).toBe('Copied!')
      expect(copyButton.classes()).toContain('copied')

      await vi.advanceTimersByTimeAsync(2000)

      expect(copyButton.text()).toBe('Copy')
      expect(copyButton.classes()).not.toContain('copied')
    } finally {
      vi.useRealTimers()
    }
  })

  it('marks only the clicked field as copied, not the others', async () => {
    const wrapper = await mountDisplay()
    const userCopyButton = manualRowFor(wrapper, 'User').get('.btn-copy')
    const hostCopyButton = manualRowFor(wrapper, 'Host').get('.btn-copy')

    await userCopyButton.trigger('click')

    expect(userCopyButton.text()).toBe('Copied!')
    expect(hostCopyButton.text()).toBe('Copy')
  })
})
