// Shared status-dot + relative-"since" formatting for the live-connections dashboard.
// Used by EntityDetailPage.vue (My services / My subscriptions), DashboardPage.vue,
// and AdminLiveConnectionsPage.vue — kept here once so the color/label/timing logic
// never has to be re-derived in more than one place.
//
// Status is split into two independent channels:
//   - `live` (dot): is traffic actually flowing for this row right now?
//   - `remoteStatus` (ring): subscriber-side rows only — the counterpart
//     (owner) entity's SSH/port state. `null`/`undefined` means no ring is
//     shown at all (service rows never carry one — see the backend's
//     `live_connections.rs` module doc comment for the full rationale).

export type RemoteStatus = 'gray' | 'orange' | 'green'

/** Dot legend: 🟢 live now · ⚪ not live right now. */
export function dotEmoji(live: boolean): string {
  return live ? '🟢' : '⚪'
}

export function dotLabel(live: boolean): string {
  return live ? 'Live now' : 'Not live right now'
}

/** Ring legend: gray = remote not connected · orange = remote connected, port not provided yet · green = remote connected and port provided. */
export const ringLabel: Record<RemoteStatus, string> = {
  gray: 'Remote is not connected',
  orange: 'Remote is connected, but this port is not provided yet',
  green: 'Remote is connected and providing this port',
}

/**
 * Formats a `connected_since` timestamp (rfc3339) as a short relative duration,
 * e.g. "2h 3m", "45s", "3d 1h". Returns "waiting…" when there's no timestamp
 * (the not-live "not connected yet" case).
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
