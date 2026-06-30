use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;
use tunnel2tunnel_core::db;
use tunnel2tunnel_ssh::{start as start_ssh, host_key_fingerprint, SshConfig};
use tunnel2tunnel_web::{start as start_http, bootstrap_admin, WebConfig};

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
    let ssh_port: u16 = std::env::var("SSH_PORT")
        .unwrap_or_else(|_| "2222".to_string())
        .parse()
        .context("SSH_PORT must be a valid port number")?;
    let admin_username = std::env::var("ADMIN_USERNAME").ok();
    let admin_password = std::env::var("ADMIN_PASSWORD").ok();
    let fail2ban_log_path = std::env::var("FAIL2BAN_LOG_PATH").ok();
    let host_key_path = std::env::var("SSH_HOST_KEY_PATH")
        .unwrap_or_else(|_| "data/ssh_host_key".to_string());
    let host_key_password = std::env::var("SSH_T2T_KEY_PASSWORD").ok();
    let static_dir = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| "frontend/dist".to_string());
    let static_dir = if std::path::Path::new(&static_dir).exists() {
        Some(static_dir)
    } else {
        None
    };

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

    let ssh_host_key_fingerprint = host_key_fingerprint(
        &host_key_path,
        host_key_password.as_deref(),
    ).context("failed to load/generate SSH host key")?;

    let http_pool = pool.clone();
    let ssh_pool = pool;

    let http = tokio::spawn(async move {
        start_http(WebConfig { http_port, ssh_port, static_dir, ssh_host_key_fingerprint }, http_pool)
            .await
            .expect("HTTP server failed")
    });

    let ssh = tokio::spawn(async move {
        start_ssh(SshConfig { ssh_port, fail2ban_log_path, host_key_path, host_key_password }, ssh_pool)
            .await
            .expect("SSH server failed")
    });

    tokio::try_join!(
        async { http.await.map_err(|e| anyhow::anyhow!("HTTP join: {e}")) },
        async { ssh.await.map_err(|e| anyhow::anyhow!("SSH join: {e}")) },
    )?;
    Ok(())
}
