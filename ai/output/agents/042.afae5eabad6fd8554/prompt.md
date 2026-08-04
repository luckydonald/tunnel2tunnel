Research codebase for planning feature (read-only, no code changes).

Feature: 
1. Status badges (online status dot, port status dot, admin "Live Connections" list) currently poll HTTP — want WebSocket realtime push instead.
2. EntityDetail page needs "copy debug data" button copying all known client/connection state as JSON to clipboard.

Investigate and report back (concise, structured):

A) Frontend
- Where do status badges live currently? Check frontend/src/components (look for status dot components), frontend/src/pages/Entities.vue, EntityDetail.vue, AdminUsers or a Dashboard/admin live connections page.
- How is current status data fetched — polling interval, which API endpoint, Pinia store involved (entities store, friends store).
- What's the exact shape of "port status" vs "online status" (ring vs dot per the recent commit "Decoupled the port-status dot from the remote's ring" — check git log/diff of commit 7d61917 for that logic).
- Where's admin "Live Connections" — likely an admin page or route; find it.
- Existing WebSocket usage in frontend (any composable, socket client lib)? Check package.json deps.

B) Backend
- Existing websocket support in tunnel2tunnel-web (axum ws extractor)? grep for "ws::" "WebSocketUpgrade" "tokio::sync::broadcast" in crates/.
- Where is connection/port state tracked in tunnel2tunnel-ssh (server slots HashMap, tunnel router) — find the source of truth for "online", "forwarding port", "live connections" (active channel splices).
- Any existing pub/sub or broadcast channel mechanism connecting ssh crate state to web crate for the admin dashboard/current polling?
- Check migrations/004_connection_logs.sql and any connection_logs query used for "Live Connections" — is it DB-polled or live from SSH state?

C) EntityDetail page
- What data is available to render already (entity, keys, ports, access rules, logs) — read EntityDetail.vue and its API calls (entities.ts) to enumerate what current client-side state could go into a debug JSON blob.
- Check existing patterns for clipboard usage in this repo (any navigator.clipboard.writeText usage elsewhere) and for `<kbd>` styling usage.

Report structure: bullet list per section A/B/C with exact file paths + line numbers/functions, and a short note on what's missing/needs building. Keep total under 600 words, no code changes.