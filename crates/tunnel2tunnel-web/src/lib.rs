mod bootstrap;
mod error;
mod extractors;
pub mod routes;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use sqlx::PgPool;
use tower_http::services::{ServeDir, ServeFile};
use tower_sessions::{cookie::time::Duration, Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;

pub use bootstrap::bootstrap_admin;
pub use error::WebError;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub ssh_port: u16,
}

pub struct WebConfig {
    pub http_port: u16,
    pub ssh_port: u16,
    /// Path to the compiled frontend dist/ directory (e.g. "frontend/dist").
    /// If None (or path doesn't exist), the SPA fallback is skipped.
    pub static_dir: Option<String>,
}

pub async fn start(config: WebConfig, pool: PgPool) -> anyhow::Result<()> {
    let state = AppState { db: pool.clone(), ssh_port: config.ssh_port };

    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await?;

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)));

    let app = Router::new()
        // auth
        .route("/api/auth/login",  post(routes::auth::login))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me",     get(routes::auth::me))
        // entities
        .route("/api/entities",
            get(routes::entities::list_entities)
            .post(routes::entities::create_entity))
        .route("/api/entities/{id}",
            get(routes::entities::get_entity)
            .put(routes::entities::update_entity)
            .delete(routes::entities::delete_entity))
        // SSH keys
        .route("/api/entities/{entity_id}/keys",
            post(routes::entities::add_key))
        .route("/api/entities/{entity_id}/keys/{key_id}",
            delete(routes::entities::delete_key))
        // ports
        .route("/api/entities/{entity_id}/ports",
            get(routes::entities::list_ports)
            .post(routes::entities::create_port))
        .route("/api/entities/{entity_id}/ports/{port_id}",
            put(routes::entities::update_port)
            .delete(routes::entities::delete_port))
        // entity access rules
        .route("/api/entities/{entity_id}/access",
            get(routes::access::list_access)
            .post(routes::access::create_access))
        .route("/api/entities/{entity_id}/access/{rule_id}",
            delete(routes::access::delete_access))
        // friends
        .route("/api/friends",
            get(routes::friends::list_friends)
            .post(routes::friends::send_request))
        .route("/api/friends/{id}",
            put(routes::friends::update_friendship))
        .route("/api/friends/{id}/grants",
            get(routes::friends::list_grants)
            .post(routes::friends::add_grant))
        .route("/api/friends/{id}/grants/{entity_id}",
            delete(routes::friends::remove_grant))
        // port discovery (client entities)
        .route("/api/entities/{entity_id}/reachable-servers",
            get(routes::entities::list_reachable_servers))
        .route("/api/entities/{client_id}/port-discovery/{server_port_id}",
            put(routes::entities::set_port_discovery_state))
        // entity connection logs
        .route("/api/entities/{id}/logs",
            get(routes::entities::list_connection_logs))
        // admin
        .route("/api/admin/users",
            get(routes::admin::list_users)
            .post(routes::admin::create_user))
        .route("/api/admin/users/{id}",
            put(routes::admin::update_user))
        // server info (authenticated)
        .route("/api/server-info",
            get(routes::server_info::get_server_info))
        // user settings
        .route("/api/me/password",
            put(routes::settings::change_password))
        .route("/api/me/ssh-keys",
            get(routes::settings::list_my_keys))
        .route("/api/me/purge-keys",
            post(routes::settings::purge_keys))
        .route("/api/me/access-rules",
            get(routes::settings::list_my_access))
        .route("/api/me/purge-access",
            post(routes::settings::purge_access))
        .layer(session_layer)
        .with_state(state);

    // Serve the compiled Vue SPA for all non-API routes
    let app = if let Some(ref dir) = config.static_dir {
        if std::path::Path::new(dir).exists() {
            let index = format!("{dir}/index.html");
            app.fallback_service(
                ServeDir::new(dir).fallback(ServeFile::new(index)),
            )
        } else {
            tracing::warn!(dir, "static dir not found; SPA not served");
            app
        }
    } else {
        app
    };

    let addr = format!("0.0.0.0:{}", config.http_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("HTTP server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
