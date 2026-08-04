**Task: report facts on entity.online source, admin route, axum ws feature — no code changes.**

1. `EntityResponse.online`/`last_disconnected_at` (crates/tunnel2tunnel-web/src/routes/entities.rs:96,98) are derived from `ConnectionLog::entity_statuses` (called at entities.rs:251), applied via `EntityResponse::with_status` (entities.rs:122-130). Not a DB column, not session_registry — same DB-backed helper `live_connections.rs` uses.

2. Route `admin-live-connections` at frontend/src/router/index.ts:78-80, only has `meta: { requiresAuth: true }` (line 80) — no admin-specific meta flag; access is presumably page/API-gated (AdminUser extractor backend-side), not enforced in the router guard (guard only checks `requiresAuth` at line 117).

3. Root Cargo.toml:24 — `axum = { version = "0.8" }`, no `features` array at all (default features only). crates/tunnel2tunnel-web/Cargo.toml:9 — `axum = { workspace = true }`, no extra features added. Axum 0.8's `ws` feature is NOT a default feature, so WebSocket support is not currently enabled.