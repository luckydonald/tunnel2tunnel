use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;
use tunnel2tunnel_core::db;
use tunnel2tunnel_web::{start, bootstrap_admin, WebConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL must be set")?;
    let http_port: u16 = std::env::var("HTTP_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .context("HTTP_PORT must be a valid port number")?;
    let admin_username = std::env::var("ADMIN_USERNAME").ok();
    let admin_password = std::env::var("ADMIN_PASSWORD").ok();

    let pool = db::connect(&database_url).await
        .context("failed to connect to database")?;

    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .context("failed to run database migrations")?;

    tracing::info!("migrations applied");

    if let (Some(username), Some(password)) = (admin_username, admin_password) {
        bootstrap_admin(&pool, &username, &password)
            .await
            .context("failed to bootstrap admin user")?;
    }

    start(WebConfig { http_port }, pool).await?;
    Ok(())
}
