import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import StatusDot from './StatusDot.vue'

describe('StatusDot', () => {
  it('renders a green (live) fill with no ring when remoteStatus is omitted', () => {
    const wrapper = mount(StatusDot, { props: { live: true } })
    expect(wrapper.classes()).toContain('self-live')
    expect(wrapper.classes()).not.toContain('ring-offline')
    expect(wrapper.classes()).not.toContain('ring-not_forwarded')
    expect(wrapper.classes()).not.toContain('ring-idle')
    expect(wrapper.classes()).not.toContain('ring-active')
    expect(wrapper.attributes('aria-label')).toBe('Live now')
  })

  it('renders a gray (not live) fill with no ring', () => {
    const wrapper = mount(StatusDot, { props: { live: false } })
    expect(wrapper.classes()).toContain('self-offline')
    expect(wrapper.classes()).not.toContain('ring-offline')
    expect(wrapper.attributes('aria-label')).toBe('Not live right now')
  })

  it('renders an offline (gray) ring when the remote is not connected — dot stays independent', () => {
    const wrapper = mount(StatusDot, { props: { live: false, remoteStatus: 'offline' } })
    expect(wrapper.classes()).toContain('self-offline')
    expect(wrapper.classes()).toContain('ring-offline')
    expect(wrapper.attributes('aria-label')).toContain('Remote is not connected')
  })

  it('renders a not_forwarded (orange) ring when the remote is connected but has not forwarded this port yet', () => {
    const wrapper = mount(StatusDot, { props: { live: false, remoteStatus: 'not_forwarded' } })
    expect(wrapper.classes()).toContain('self-offline')
    expect(wrapper.classes()).toContain('ring-not_forwarded')
    expect(wrapper.attributes('aria-label')).toContain('not forwarded yet')
  })

  it('renders an idle (blue) ring when the port is forwarded but nothing is bridged right now', () => {
    const wrapper = mount(StatusDot, { props: { live: false, remoteStatus: 'idle' } })
    expect(wrapper.classes()).toContain('self-offline')
    expect(wrapper.classes()).toContain('ring-idle')
    expect(wrapper.attributes('aria-label')).toContain('nothing is bridged')
  })

  it('renders a live green fill alongside an active (green) ring when both are true', () => {
    const wrapper = mount(StatusDot, { props: { live: true, remoteStatus: 'active' } })
    expect(wrapper.classes()).toContain('self-live')
    expect(wrapper.classes()).toContain('ring-active')
    expect(wrapper.attributes('aria-label')).toBe(
      'Live now · Actively bridging traffic through this port right now',
    )
  })

  it('prefers selfStatus over the boolean live fill when both are given', () => {
    const wrapper = mount(StatusDot, { props: { live: true, selfStatus: 'not_forwarded' } })
    expect(wrapper.classes()).toContain('self-not_forwarded')
    expect(wrapper.classes()).not.toContain('self-live')
    expect(wrapper.attributes('aria-label')).toBe('Connected, but not forwarding this port yet')
  })
})
