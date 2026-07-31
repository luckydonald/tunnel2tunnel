import { describe, it, expect, vi, afterEach } from 'vitest'
import { formatSince, statusDotEmoji, statusDotLabel } from '@/liveStatus'

describe('statusDotEmoji / statusDotLabel', () => {
  it('has an entry for every status', () => {
    expect(statusDotEmoji.green).toBe('🟢')
    expect(statusDotEmoji.gray).toBe('⚪')
    expect(statusDotEmoji.orange).toBe('🟠')
    expect(statusDotLabel.green).toBeTruthy()
    expect(statusDotLabel.gray).toBeTruthy()
    expect(statusDotLabel.orange).toBeTruthy()
  })
})

describe('formatSince', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns "waiting…" for null/undefined', () => {
    expect(formatSince(null)).toBe('waiting…')
    expect(formatSince(undefined)).toBe('waiting…')
  })

  it('returns "waiting…" for an unparseable timestamp', () => {
    expect(formatSince('not-a-date')).toBe('waiting…')
  })

  it('formats seconds-only durations', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T00:00:45Z'))
    expect(formatSince('2026-01-01T00:00:00Z')).toBe('45s')
  })

  it('formats minutes + seconds', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T00:12:03Z'))
    expect(formatSince('2026-01-01T00:00:00Z')).toBe('12m 3s')
  })

  it('formats hours + minutes', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T02:03:00Z'))
    expect(formatSince('2026-01-01T00:00:00Z')).toBe('2h 3m')
  })

  it('formats days + hours', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-04T01:00:00Z'))
    expect(formatSince('2026-01-01T00:00:00Z')).toBe('3d 1h')
  })
})
