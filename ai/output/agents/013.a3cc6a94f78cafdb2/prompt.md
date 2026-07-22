In repo /home/user/git/luckydonald/tunnel2tunnel, I need a full understanding of the existing "connection_logs" feature end to end, since I'm designing a new similar admin-facing paginated/filterable log table (for SSH auth-attempt / tarpit events).

Explore:
1. migrations/004_connection_logs.sql — full schema, quote it.
2. tunnel2tunnel-core: the Rust model/struct for connection logs, and any query functions (insert, list with pagination/filter/search) — quote file paths, line numbers, and code.
3. tunnel2tunnel-web: the API route(s) exposing connection logs to the admin (routes/*.rs) — quote the handler code, query param parsing (pagination, filter, search), and response shape.
4. frontend: how connection logs (or entity logs, /api/entities/{id}/logs) are rendered in the Vue admin UI — which page/component, how pagination/filter/search UI works, quote relevant snippets. Also check labels.ts for any enum label patterns used for these logs.
5. Settings page pattern: how do admin-settable rules/config get stored and edited via the webui currently (e.g. IP whitelist settings, or any other admin-configurable rule list) — I need to add new admin-settable tarpit/ban rules (by peer_ip or by user). Look at tunnel2tunnel-core's `settings` table usage, and the Settings.vue page, and any existing "rules list" UI pattern (like entity_access rules editing) to reuse.
6. How enums are represented across DB (sqlx) <-> Rust <-> TypeScript today (e.g. entity subject_type, visibility_grant) — quote an example of the full stack for one enum, since I'll need a similar enum for "tarpit method" and "fail reason"/"success reason".

Report file paths, line numbers, and quoted code. Research only, no modifications.