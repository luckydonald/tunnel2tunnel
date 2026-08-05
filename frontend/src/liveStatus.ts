// Shared status-dot + relative-"since" formatting for the live-connections dashboard.
// Used by EntityDetailPage.vue (My services / My subscriptions), DashboardPage.vue,
// and AdminLiveConnectionsPage.vue — kept here once so the color/label/timing logic
// never has to be re-derived in more than one place.
//
// Status is split into two channels that share the same 4-state semantics
// on both service (owner) and subscription (subscriber) rows:
//   - `live` (dot): is traffic actually flowing for this row right now?
//   - `remoteStatus` (ring): the counterpart's (owner, for a subscription
//     row; this entity itself, for a service row) SSH/port state — see the
//     backend's `live_connections.rs` module doc comment for the full
//     rationale.

export type RemoteStatus = 'offline' | 'not_forwarded' | 'idle' | 'active'

/** Dot legend: 🟢 live now · ⚪ not live right now. */
export function dotEmoji(live: boolean): string {
  return live ? '🟢' : '⚪'
}

export function dotLabel(live: boolean): string {
  return live ? 'Live now' : 'Not live right now'
}

/** Ring legend: offline = not connected · not_forwarded = connected but port not forwarded yet · idle = forwarded but nothing bridged right now · active = actively bridging traffic. */
export const ringLabel: Record<RemoteStatus, string> = {
  offline: 'Remote is not connected',
  not_forwarded: 'Remote is connected, but this port is not forwarded yet',
  idle: 'Port is forwarded, but nothing is bridged through it right now',
  active: 'Actively bridging traffic through this port right now',
}

/**
 * Same 4-state semantics as `ringLabel`, worded from this side's own point
 * of view instead of the counterpart's — used when a row's dot (not just
 * its ring) needs to show a 4-state fill, e.g. a service row's own
 * forwarding state (see `StatusDot`'s `selfStatus` prop).
 */
export const selfLabel: Record<RemoteStatus, string> = {
  offline: 'Not connected',
  not_forwarded: 'Connected, but not forwarding this port yet',
  idle: 'Forwarding this port, but nothing is bridged through it right now',
  active: 'Actively bridging traffic through this port right now',
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
