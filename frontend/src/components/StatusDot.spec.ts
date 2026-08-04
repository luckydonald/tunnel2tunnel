import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import StatusDot from './StatusDot.vue'

describe('StatusDot', () => {
  it('renders the green (live) emoji with no ring when remoteStatus is omitted', () => {
    const wrapper = mount(StatusDot, { props: { live: true } })
    expect(wrapper.text()).toContain('🟢')
    expect(wrapper.classes()).not.toContain('ring-gray')
    expect(wrapper.classes()).not.toContain('ring-orange')
    expect(wrapper.classes()).not.toContain('ring-green')
    expect(wrapper.attributes('aria-label')).toBe('Live now')
  })

  it('renders the gray (not live) emoji with no ring', () => {
    const wrapper = mount(StatusDot, { props: { live: false } })
    expect(wrapper.text()).toContain('⚪')
    expect(wrapper.classes()).not.toContain('ring-gray')
    expect(wrapper.attributes('aria-label')).toBe('Not live right now')
  })

  it('renders a gray ring when the remote is not connected — dot stays independent', () => {
    const wrapper = mount(StatusDot, { props: { live: false, remoteStatus: 'gray' } })
    expect(wrapper.text()).toContain('⚪')
    expect(wrapper.classes()).toContain('ring-gray')
    expect(wrapper.attributes('aria-label')).toContain('Remote is not connected')
  })

  it('renders an orange ring when the remote is connected but has not provided this port yet', () => {
    const wrapper = mount(StatusDot, { props: { live: false, remoteStatus: 'orange' } })
    expect(wrapper.text()).toContain('⚪')
    expect(wrapper.classes()).toContain('ring-orange')
    expect(wrapper.attributes('aria-label')).toContain('not provided yet')
  })

  it('renders a live green dot alongside a green ring when both are true', () => {
    const wrapper = mount(StatusDot, { props: { live: true, remoteStatus: 'green' } })
    expect(wrapper.text()).toContain('🟢')
    expect(wrapper.classes()).toContain('ring-green')
    expect(wrapper.attributes('aria-label')).toBe('Live now · Remote is connected and providing this port')
  })
})
