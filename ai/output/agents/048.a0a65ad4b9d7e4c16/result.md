I have a complete picture now. Here's the report.

## 1. `composables/useLiveSocket.ts` (full file, 65 lines)

Generic reconnecting WebSocket composable, no domain knowledge of message types:

```ts
export function useLiveSocket<T>(path: string, onMessage: (data: T) => void): void
```

- Builds `ws(s)://<host><path>` from `location.protocol`/`location.host`, cookies ride along automatically (same-origin).
- On `message`, does `JSON.parse(event.data)` and calls `onMessage(data)` — **no message envelope/type discrimination at all**. Each socket only ever carries one payload shape (the generic `T`), pushed wholesale on every server update — it's not an event stream with distinct event types, just "full current state, re-pushed".
- Reconnects with exponential backoff (1000ms → 15000ms cap) on `close`; logs warnings on `error`. Cleans up (`stopped=true`, clears timer, closes socket) in `onUnmounted`.
- There is no distinction today between "initial snapshot" vs "delta/event" messages, and no message metadata (no `type` field, no timestamp, no id) — just the raw `T` payload each time. Any toast-worthy "event" (e.g., "went from idle to active") would have to be **derived by the consumer** by diffing the new payload against the previous local `ref` value before overwriting it.

## 2. Three consumers of `useLiveSocket`, three distinct payload shapes

All three call it as `useLiveSocket<T>(path, data => { ...assign to local refs... })` and just overwrite state — none currently diff previous vs. new value.

- **`pages/DashboardPage.vue`** (33 lines total, read in full)
  - `useLiveSocket<DashboardRow[]>('/api/me/live-connections/ws', data => { rows.value = data; loading.value = false })` (lines 29-32)
  - `DashboardRow` (defined inline, lines 12-21): `{ live: boolean; remote_status: RemoteStatus | null; entity_id: string; entity_name: string | null; role: 'server'|'client'; service_name: string; port: number; connected_since: string | null }`
  - Renders `<StatusDot :live="row.live" :remote-status="row.remote_status" />` per row (line 61).

- **`pages/AdminLiveConnectionsPage.vue`** (lines 33-36): `useLiveSocket<LiveConnectionRow[]>('/api/admin/live-connections/ws', ...)`. `LiveConnectionRow` type in `frontend/src/api/admin.ts:118-128`: `{ live, remote_status: RemoteStatus | null, account: LiveAccountRef, entity: LiveEntityRef, role, service_name, port, peer_ip, connected_since }`.

- **`pages/EntityDetailPage.vue`** (lines 68-84): `useLiveSocket<EntityLiveMessage>(`/api/entities/${entityId}/live-connections/ws`, ...)` where `EntityLiveMessage extends EntityLiveConnectionsResponse` plus `entity_online: boolean` and `entity_last_disconnected_at: string | null`. `EntityLiveConnectionsResponse` (`api/entities.ts:113-116`) = `{ services: ServiceLiveStatus[], subscriptions: SubscriptionLiveStatus[] }`.
  - `ServiceLiveStatus` (`api/entities.ts:89-98`): `{ port_config_id, service_name, proxy_port, live, remote_status: RemoteStatus | null, subscribers: LiveSubscriberInfo[] }`
  - `SubscriptionLiveStatus` (`api/entities.ts:100-111`): `{ subscription_id, port_config_id, owner: LiveEntityRef, service_name, proxy_port, subscriber_local_port, enabled, live, remote_status: RemoteStatus | null, connected_since }`
  - This page already imports `useToast` too (line 26, `const { show: toast } = useToast()`) but currently only uses it elsewhere in the file for form-submission errors, not for the live-socket callback.

All four `RemoteStatus`-bearing shapes share the same status vocabulary defined in `liveStatus.ts:14`: `type RemoteStatus = 'offline' | 'not_forwarded' | 'idle' | 'active'`.

## 3. Existing toast/notification system — **already exists**

- **`frontend/src/composables/useToast.ts`** (28 lines, full file read) — a small singleton store (module-scope `ref` shared across all `useToast()` callers, not per-component state):
  ```ts
  export type ToastLevel = 'error' | 'info' | 'success'
  export function useToast() {
    function show(message: string, level: ToastLevel = 'error', durationMs = 6000): void
    function dismiss(id: number): void
    return { toasts, show, dismiss }
  }
  ```
  Default level is `'error'`, default duration 6000ms, auto-dismiss via `setTimeout`.
- **`frontend/src/components/ToastContainer.vue`** (full file read) — renders `toasts` via `Teleport to="body"`, fixed bottom-right stack, styled per level (`.error`/`.info`/`.success`), click-to-dismiss.
- Already used in: `pages/EntitiesPage.vue`, `pages/FriendsPage.vue`, `pages/EntityDetailPage.vue` (pattern: `const { show: toast } = useToast()` then `toast('message', 'error'|'info'|'success')`).
- Mounted globally exactly once, in **`components/AppShell.vue:65`**: `<ToastContainer />`, as a sibling of the `.shell` div (after the closing tag), so it's already a proper global overlay independent of route content. `AppShell.vue` is used by every authenticated page (Dashboard, EntityDetail, AdminLiveConnections, etc. all wrap content in `<AppShell>...</AppShell>`), so any toast fired from those pages' composables will render through the existing container without extra wiring.

**No external toast library** (no vue-toastification, vue3-toastify, vue-sonner, naive-ui, etc.) is installed or referenced anywhere.

## 4. `package.json` dependencies

Confirmed via `/home/user/git/luckydonald/tunnel2tunnel/frontend/package.json`: only `@sentry/vue`, `pinia`, `vue`, `vue-router` as runtime deps — no toast library dependency. The existing toast system is fully homegrown.

## 5. `labels.ts` — status label conventions

`frontend/src/labels.ts` has no `RemoteStatus` labels itself (those live in `liveStatus.ts`, not `labels.ts`), but shows the project's `Record<Enum, string>` + `.map` "Options" pattern used everywhere for human-readable enum labels (e.g. `roleBadgeLabel`, `tarpitMethodLabel`, `banScopeTypeLabel`, `failReasonLabel` at lines 86-100, `successReasonLabel` at 102-105). The actual live-status text lives in **`frontend/src/liveStatus.ts`**:
- `dotEmoji(live)` → 🟢/⚪, `dotLabel(live)` → `'Live now'` / `'Not live right now'` (lines 17-23)
- `ringLabel: Record<RemoteStatus, string>` (lines 26-31): `offline: 'Remote is not connected'`, `not_forwarded: 'Remote is connected, but this port is not forwarded yet'`, `idle: 'Port is forwarded, but nothing is bridged through it right now'`, `active: 'Actively bridging traffic through this port right now'`.
- `formatSince(connectedSince)` (lines 38-55) for relative "Xh Ym"/"waiting…" display.

These `ringLabel`/`dotLabel` strings are exactly the human-readable text a toast for a status transition (e.g. "went offline" → "active") should reuse for consistency, rather than inventing new copy.

## 6. `components/` and `App.vue`/`AppShell.vue` structure

- **`App.vue`** (full file, 9 lines) is minimal — just `<RouterView :key="route.path" />`, no global mounts here.
- **`components/AppShell.vue`** (full file, 228 lines) is the real app chrome: sidebar nav + `<main class="content"><slot /></main>` + `<ToastContainer />` as a top-level sibling (line 65). This is the single global mount point/portal pattern already established in this codebase — any new global singleton UI (e.g., if toasts needed a second container or a different overlay) should follow this same convention (mount once in `AppShell.vue`, use `Teleport to="body"` inside the component itself for actual DOM placement).
- `components/StatusDot.vue` (full file, 56 lines) is the visual analog already showing `dotEmoji`/`ringLabel` as `:title`/`aria-label` tooltips — reusable reference for what text a toast should mirror.

## Summary for the plan

- Toast infra is 100% ready to use as-is (`useToast().show(message, level)`); no new library/component needed.
- `useLiveSocket` gives no event semantics — it's "latest full state snapshot," so toast-triggering logic must live in each consumer (`DashboardPage.vue`, `AdminLiveConnectionsPage.vue`, `EntityDetailPage.vue`), diffing incoming `RemoteStatus`/`live`/`entity_online` values against the previously-stored ref before overwriting, then calling `toast(...)` using `ringLabel`/`dotLabel` text from `liveStatus.ts` for consistent copy.
- Three separate sockets/shapes to instrument if toasts are wanted dashboard-wide: `/api/me/live-connections/ws` (`DashboardRow[]`), `/api/admin/live-connections/ws` (`LiveConnectionRow[]`), `/api/entities/:id/live-connections/ws` (`EntityLiveMessage` with nested `services`/`subscriptions` arrays plus `entity_online`/`entity_last_disconnected_at`) — each needs its own prior-state comparison since the array shapes/keys (`entity_id` vs `port_config_id`/`subscription_id`) differ.