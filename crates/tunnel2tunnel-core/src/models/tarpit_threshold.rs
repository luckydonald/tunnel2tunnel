use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TarpitThreshold {
    pub id: Uuid,
    pub fail_count: i32,
    pub window_seconds: i64,
    pub enabled: bool,
    pub action: String,
    #[sqlx(flatten)]
    pub ts: Timestamps,
}

impl TarpitThreshold {
    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, TarpitThreshold>(
            "SELECT * FROM tarpit_thresholds ORDER BY window_seconds ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Enabled rows only — what the SSH-side tarpit engine actually enforces.
    pub async fn list_enabled(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, TarpitThreshold>(
            "SELECT * FROM tarpit_thresholds WHERE enabled ORDER BY window_seconds ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        fail_count: i32,
        window_seconds: i64,
        enabled: bool,
        action: &str,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, TarpitThreshold>(
            "INSERT INTO tarpit_thresholds (id, fail_count, window_seconds, enabled, action) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING *",
        )
        .bind(id)
        .bind(fail_count)
        .bind(window_seconds)
        .bind(enabled)
        .bind(action)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        fail_count: i32,
        window_seconds: i64,
        enabled: bool,
        action: &str,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, TarpitThreshold>(
            "UPDATE tarpit_thresholds \
             SET fail_count = $2, window_seconds = $3, enabled = $4, action = $5 \
             WHERE id = $1 \
             RETURNING *",
        )
        .bind(id)
        .bind(fail_count)
        .bind(window_seconds)
        .bind(enabled)
        .bind(action)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query("DELETE FROM tarpit_thresholds WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
