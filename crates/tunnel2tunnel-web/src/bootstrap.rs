use sqlx::PgPool;
use tunnel2tunnel_core::{models::user::User, CoreError};

pub async fn bootstrap_admin(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<(), CoreError> {
    if User::exists_by_username(pool, username).await? {
        tracing::info!("admin '{username}' already exists — skipping bootstrap");
        return Ok(());
    }
    User::create(
        pool,
        username,
        None,
        password,
        true,
        Some("Bootstrapped admin"),
    )
    .await?;
    tracing::info!("created admin user '{username}'");
    Ok(())
}
