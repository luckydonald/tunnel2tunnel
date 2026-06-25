mod bootstrap;
mod error;
mod extractors;
pub mod routes;

use axum::{routing::{get, post}, Router};
use sqlx::PgPool;
use tower_sessions::{cookie::time::Duration, Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;

pub use bootstrap::bootstrap_admin;
pub use error::WebError;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

pub struct WebConfig {
    pub http_port: u16,
}

pub async fn start(config: WebConfig, pool: PgPool) -> anyhow::Result<()> {
    let state = AppState { db: pool.clone() };

    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await?;

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)));

    let app = Router::new()
        .route("/api/auth/login", post(routes::auth::login))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me",     get(routes::auth::me))
        .layer(session_layer)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.http_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("HTTP server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
