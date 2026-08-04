labels.ts has no status-dot related strings — those all live in liveStatus.ts. Here's the report:

**Where subscriber-side status dots render**
- `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/EntityDetailPage.vue:559-563` — `<StatusDot :live="..." :remote-status="serviceStatusMap.get(port.id)!.remote_status" />` (renders for the entity's own ports/subscribers list)
- `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/components/ServiceConnector.vue:141` — `<StatusDot ... :remote-status="statusFor(service.subscription.id)!.remote_status" />` (subscriber's own subscription list)
- `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/DashboardPage.vue:61` and `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/AdminLiveConnectionsPage.vue:78` also use the same `StatusDot`.

All logic/text is centralized in `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/liveStatus.ts` and consumed by `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/components/StatusDot.vue`. `frontend/src/labels.ts` has no involvement (no matching keys/strings there).

**(a) `remote_status` ring — states are `gray | orange | green`, no "yellow"** (`frontend/src/liveStatus.ts:13,25-29`):
- `gray` → `"Remote is not connected"`
- `orange` → `"Remote is connected, but this port is not provided yet"`
- `green` → `"Remote is connected and providing this port"`

**(b) `live` boolean dot** (`frontend/src/liveStatus.ts:16-22`):
- `dotEmoji`: `live ? '🟢' : '⚪'`
- `dotLabel`: `live ? 'Live now' : 'Not live right now'`

**Combined tooltip/aria-label construction** (`frontend/src/components/StatusDot.vue:11-15`):
```
label = [dotLabel(live), ringLabel[remoteStatus]].join(' · ')
```
e.g. `"Live now · Remote is connected and providing this port"` or `"Not live right now · Remote is not connected"`. If `remoteStatus` is null/undefined, only the dot label is shown (no ring, no " · " suffix) — this is the case for "service rows" per the comment at `frontend/src/liveStatus.ts:9-11`.

This `title`/`aria-label` is set on the `<span class="status-dot">` element itself in `StatusDot.vue:20-27`.