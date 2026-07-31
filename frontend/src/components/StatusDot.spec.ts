import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import StatusDot from './StatusDot.vue'

describe('StatusDot', () => {
  it('renders the green emoji with an accessible label', () => {
    const wrapper = mount(StatusDot, { props: { status: 'green' } })
    expect(wrapper.text()).toContain('🟢')
    expect(wrapper.attributes('aria-label')).toBe('Live now')
  })

  it('renders the gray emoji', () => {
    const wrapper = mount(StatusDot, { props: { status: 'gray' } })
    expect(wrapper.text()).toContain('⚪')
  })

  it('renders the orange emoji', () => {
    const wrapper = mount(StatusDot, { props: { status: 'orange' } })
    expect(wrapper.text()).toContain('🟠')
  })
})
