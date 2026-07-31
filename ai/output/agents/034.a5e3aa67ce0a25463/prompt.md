Repo: /home/user/git/luckydonald/tunnel2tunnel, `frontend/` directory (Vue 3 + `<script setup lang="ts">` + TypeScript strict + SCSS, Vite, Pinia, Vue Router). Read CLAUDE.md at repo root first, and `ai/skills/code-style/references/vue.md`.

This is the final piece of a larger redesign. Plan: `/home/user/.confuig/claude/accounts/private/plans/robust-bubbling-ember.md` — read it in full, especially "Live connections dashboard" and the Dashboard/Admin GUI mockups near the end. Everything else in that plan (schema, SSH enforcement, web routes, and the frontend My-services/My-subscriptions/Entities-list restructure) is already committed on `mane` — check `git log --oneline -15` and read the CURRENT state of `frontend/src/pages/EntityDetailPage.vue`, `frontend/src/components/ServiceConnector.vue`, `frontend/src/api/entities.ts`, and `frontend/src/pages/EntitiesPage.vue` as they exist now (they were rebuilt in prior commits — don't assume the plan's file descriptions are still 100% accurate to current structure, verify by reading).

## Backend API you're building against (already implemented and committed as `526b49b`)

**`GET /api/entities/{id}/live-connections`** (owner-only) → `EntityLiveConnectionsResponse`:
```ts
{
  services: Array<{
    port_config_id: string, service_name: string, proxy_port: number,
    status: "green" | "gray",
    subscribers: Array<{
      entity: { id: string, name: string | null },
      account: { user_id: string, username: string },
      peer_ip: string,
      connected_since: string  // rfc3339
    }>
  }>,
  subscriptions: Array<{
    subscription_id: string, port_config_id: string,
    owner: { id: string, name: string | null },
    service_name: string, proxy_port: number, subscriber_local_port: number,
    enabled: boolean,
    status: "green" | "gray" | "orange",
    connected_since: string | null  // rfc3339
  }>
}
```

**`GET /api/admin/live-connections`** (admin-only) → flat array, one row per leg:
```ts
Array<{
  status: "green" | "gray" | "orange",
  account: { user_id: string, username: string },
  entity: { id: string, name: string | null },
  role: "server" | "client",
  service_name: string,
  port: number,  // proxy_port for role:"server", subscriber_local_port for role:"client"
  peer_ip: string | null,
  connected_since: string | null
}>
```

## What to build

1. **Status-dot column on the entity detail page** (`frontend/src/pages/EntityDetailPage.vue` and/or `ServiceConnector.vue`, whichever currently owns the "My services"/"My subscriptions" rendering — read both first): fetch `GET /api/entities/{id}/live-connections` alongside the existing data loads, and:
   - "My services" table: add the status dot (🟢/⚪, per `services[].status`) as the first column per the mockup, and render the connected-subscribers list per row (account/entity/peer_ip/since) — likely an expandable row or a small inline sub-list, matching the mockup's `▸ Postgres subscribers: my-laptop (alice), local port 5433, 12m` style (note: the backend's `subscribers[]` doesn't include the subscriber's own local port in this shape — check if it's derivable from data you already have client-side, e.g. from the subscriber's own `port_subscriptions` if that's ever visible to you, otherwise just omit that detail and show account/entity/since, which the backend does provide — don't fabricate it).
   - "My subscriptions" section (in `ServiceConnector.vue` or wherever it now lives): add the status dot (🟢/⚪/🟠, per `subscriptions[].status`, matched by `subscription_id` or `port_config_id` to the existing subscription rows) as the first column/marker per row, and for the 🟠 case surface the "server is offline — will connect automatically once it's back" hint text from the mockup.
   - Status-dot legend/colors: 🟢 green, ⚪ gray, 🟠 orange — implement as a small reusable piece (a tiny `StatusDot.vue` component or a shared class/helper) since it's needed in 3 places (this page, Dashboard, admin page) — don't triplicate the color logic.

2. **`frontend/src/pages/DashboardPage.vue`** — currently a near-empty stub (just a welcome message). Replace with a real live view per the mockup: fetch live-connections for the current user's own entities and render a flat table (dot, entity, role badge — reuse the 🖧/💻 badges already built for the Entities list — service, port, since). Since there's no single "my own everything" convenience endpoint (only per-entity `GET .../live-connections`), fetch the user's own entities (reuse whatever the Entities list already uses, e.g. `entitiesApi.list()`) and call `GET .../live-connections` per owned entity, flattening `services`+`subscriptions` into role-tagged rows (`role: "server"` from `services[]`, `role: "client"` from `subscriptions[]`). If that N+1-per-page-load pattern feels too heavy, it's still the correct approach given the current API surface — just say so in your report if you think a follow-up aggregate endpoint would help, don't invent one client-side. Include a "See all →" link to the new admin page (only rendered for admin users — check how other admin-only nav items are conditionally shown elsewhere, e.g. `AppShell.vue` or `AdminUsers.vue`'s route guard, and mirror that).

3. **New `frontend/src/pages/AdminLiveConnections.vue`** — follow the existing admin-page conventions (check `frontend/src/pages/AdminUsers.vue` and/or `PurgeAccess.vue`/`PurgeKeys.vue` for the established layout/auth-guard/API-call pattern and mirror it closely, including router registration with the same admin-only guard). Fetch `GET /api/admin/live-connections`, render the flat table per the mockup: status dot, account, entity (linking to its detail page), role badge (🖧/💻 reused), service name, port, peer IP (omit column entirely if you find rows commonly have `null` there and it looks better omitted — your call, but don't render "null" as text), connected-since (relative time — check if the repo already has a relative-time formatting helper anywhere in `frontend/src/`, e.g. used by the existing Connection Log section, and reuse it rather than writing a new one). Add basic client-side filters if reasonably easy (by role/account/service) per the mockup's `Filter: [ user ▾ ] [ role ▾ ] [ service ▾ ]` — but don't over-engineer this if the existing admin pages don't have an established filter-UI pattern to follow; a simple text/select filter over the already-fetched array is enough, no need for server-side filtering params.

4. Add this new admin page to the nav (wherever `AdminUsers.vue`/`PurgeAccess.vue` etc. are linked from — likely `AppShell.vue`).

## Notes

- No status-color logic should live in more than one place — build the shared dot component/helper first, use it in all three surfaces.
- Relative "since" formatting (e.g. "2h 3m", "waiting…" for gray/orange rows with no `connected_since`) should also be a single shared helper, reused across all three surfaces.
- This repo's CLAUDE.md requires enum-like values to go through `frontend/src/labels.ts` conventions — `status`/`role` here are simple enough that a small local const map is fine, but check if there's an obvious existing pattern (e.g. how the 🖧/💻 role badges were implemented in the earlier commit) and match its style rather than inventing a different convention.

## Verification

- `npm run build` must pass clean (strict TS, no `any`).
- Add tests for new components per this repo's established Vue testing conventions (check existing `.spec.ts` files, e.g. `ServiceConnector.spec.ts` from the prior phase, for the pattern to follow) — at minimum for the shared status-dot/relative-time helpers and the new `AdminLiveConnections.vue`/`DashboardPage.vue` data-flattening logic.
- No running backend is likely available in this environment; rely on `npm run build` + unit tests as the primary gate, and say so explicitly if you can't do a live browser walkthrough.

## Committing

`commit-with-lplp-style` skill is active — write messages to `ai/git/pending-commit.md` first, format `[frontend] topic: ai: Run: <summary>.`, stage only files you changed by explicit path (never `git add -A`/`.`). A few natural-checkpoint commits (e.g. shared status/time helpers + entity-detail status dots, then Dashboard, then admin page + nav) is fine.

Report back: what you built, any deviations and why, and final `npm run build`/test status.