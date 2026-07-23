use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::TimestampsSoftDelete;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TarpitThreshold {
    pub id: Uuid,
    pub fail_count: i32,
    pub window_seconds: i64,
    pub enabled: bool,
    pub action: String,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: TimestampsSoftDelete,
}

impl TarpitThreshold {
    /// All rows, including soft-deleted ones — used by the admin CRUD listing so old
    /// connection_logs references still resolve to a visible (if deleted) row.
    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, TarpitThreshold>(
            "SELECT * FROM tarpit_thresholds ORDER BY window_seconds ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Enabled, non-deleted rows only — what the SSH-side tarpit engine actually enforces.
    pub async fn list_enabled(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, TarpitThreshold>(
            "SELECT * FROM tarpit_thresholds WHERE enabled AND deleted_at IS NULL ORDER BY window_seconds ASC",
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

    pub async fn soft_delete(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE tarpit_thresholds SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn restore(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE tarpit_thresholds SET deleted_at = NULL WHERE id = $1 AND deleted_at IS NOT NULL",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
