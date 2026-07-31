I completed the live-connections dashboard frontend work described in the task.

**What was built:**

1. **Shared helpers** (`frontend/src/liveStatus.ts`, `frontend/src/components/StatusDot.vue`) — single source of truth for the 🟢/⚪/🟠 status-dot coloring and the `formatSince()` relative-time formatter, used by all three surfaces.

2. **Entity detail page** (`frontend/src/pages/EntityDetailPage.vue`, `frontend/src/components/ServiceConnector.vue`) — "My services" table now has a status-dot column and an expandable `▸ N connected` subscribers row (account/entity/peer IP/since — subscriber's own local port omitted since the backend doesn't expose it, per the task's instruction not to fabricate it). "My subscriptions" gets a status dot per row plus the orange "server is offline — will connect automatically once it's back" hint.

3. **`frontend/src/pages/DashboardPage.vue`** — replaced the stub with a real table: fetches the user's own entities, calls `GET .../live-connections` per entity, flattens into role-tagged rows. This is the N+1-per-page-load pattern the task anticipated; I left a code comment flagging it and noting a future aggregate endpoint would help, rather than inventing one. Includes an admin-only "See all →" link.

4. **New `frontend/src/pages/AdminLiveConnectionsPage.vue`** — mirrors `AdminUsersPage.vue`/`AdminConnectionLogsPage.vue` conventions, flat one-row-per-leg table with client-side user/role/service filters. The Peer IP column is dropped entirely when no fetched row has one (rather than showing a column of "—").

5. Router (`admin-live-connections` route) and `AppShell.vue` nav link updated; `api/entities.ts`/`api/admin.ts` gained the new response types and API calls.

**Deviations:** none significant — followed existing conventions closely (dark theme colors, admin page layout, `roleBadgeLabel` reuse).

**Verification:** `npm run build` passes clean (strict TS, no `any`). Added tests for `liveStatus.ts`, `StatusDot.vue`, `DashboardPage.vue`'s flattening/admin-link logic, and `AdminLiveConnectionsPage.vue`'s peer-IP-column/filter logic — full suite is 39/39 passing. No backend was running in this environment, so there was no live browser walkthrough; build + unit tests were the verification gate as anticipated in the task.

Committed as `ad3a6d0` on `mane`, after folding two stray `ai:` auto-commits (agent-result/prompt records) that had landed on top of the prior backend commit.

result: Built and committed the live-connections dashboard frontend (shared status-dot/relative-time helpers, entity-detail status dots + subscriber list, rebuilt Dashboard page, new admin Live Connections page + nav/route) — `npm run build` and the full test suite (39 tests) both pass, commit `ad3a6d0` on `mane`.