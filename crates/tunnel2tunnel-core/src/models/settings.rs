use crate::error::CoreError;
use sqlx::PgPool;

pub struct Settings;

impl Settings {
    pub async fn get(pool: &PgPool, key: &str) -> Result<Option<String>, CoreError> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(CoreError::Sqlx)?;
        Ok(row.map(|(v,)| v))
    }

    pub async fn set(pool: &PgPool, key: &str, value: &str) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(())
    }
}
