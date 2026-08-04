Read-only research, report concisely (under 250 words), no code changes.

1. In crates/tunnel2tunnel-web/src/routes/entities.rs find get_entity / list_entities handlers — how are `online` and `last_disconnected_at` fields on `Entity`/`EntityDetail` computed? Is it derived from `tunnel2tunnel_core::models::connection_log::ConnectionLog::entity_statuses` (same helper live_connections.rs uses) or something else (e.g. a column on the entities table, or session_registry)? Give exact file:line.

2. In frontend/src/router (likely frontend/src/router/index.ts) find the route name/path for the admin live connections page (referenced as `admin-live-connections` in DashboardPage.vue) and confirm it requires admin meta.

3. Check crates/tunnel2tunnel-web/Cargo.toml (or workspace Cargo.toml) for whether axum's "ws" feature is already enabled for the axum dependency (needed for WebSocketUpgrade extractor).

Report just the facts with file:line references, no recommendations.