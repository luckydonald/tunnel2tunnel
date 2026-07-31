// Shared status-dot + relative-"since" formatting for the live-connections dashboard.
// Used by EntityDetailPage.vue (My services / My subscriptions), DashboardPage.vue,
// and AdminLiveConnectionsPage.vue — kept here once so the color/label/timing logic
// never has to be re-derived in more than one place.

export type LiveStatus = 'green' | 'gray' | 'orange'

/** Status-dot legend: 🟢 live now · ⚪ configured/available, not live · 🟠 subscribed, but the other side isn't live yet. */
export const statusDotEmoji: Record<LiveStatus, string> = {
  green: '🟢',
  gray: '⚪',
  orange: '🟠',
}

export const statusDotLabel: Record<LiveStatus, string> = {
  green: 'Live now',
  gray: 'Configured, not live',
  orange: 'Waiting for the other side to come online',
}

/**
 * Formats a `connected_since` timestamp (rfc3339) as a short relative duration,
 * e.g. "2h 3m", "45s", "3d 1h". Returns "waiting…" when there's no timestamp
 * (the gray/orange "not connected yet" case).
 */
export function formatSince(connectedSince: string | null | undefined): string {
  if (!connectedSince) return 'waiting…'

  const since = new Date(connectedSince).getTime()
  if (Number.isNaN(since)) return 'waiting…'

  const totalSeconds = Math.max(0, Math.floor((Date.now() - since) / 1000))

  const days = Math.floor(totalSeconds / 86400)
  const hours = Math.floor((totalSeconds % 86400) / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60

  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${minutes}m`
  if (minutes > 0) return `${minutes}m ${seconds}s`
  return `${seconds}s`
}
