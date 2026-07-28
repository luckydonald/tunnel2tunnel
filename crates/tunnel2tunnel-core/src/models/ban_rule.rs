use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::TimestampsSoftDelete;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct BanRule {
    pub id: Uuid,
    pub scope_type: String,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub active_until: Option<OffsetDateTime>,
    pub created_by: Uuid,
    pub action: String,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: TimestampsSoftDelete,
}

impl BanRule {
    /// Rules currently in effect (no expiry, or expiry still in the future) and not soft-deleted.
    pub async fn list_active(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, BanRule>(
            "SELECT * FROM ban_rules \
             WHERE (active_until IS NULL OR active_until > NOW()) AND deleted_at IS NULL \
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// All rules, including expired/soft-deleted ones — used by the admin CRUD listing so old
    /// connection_logs references still resolve to a visible (if deleted) row.
    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, BanRule>("SELECT * FROM ban_rules ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
            .map_err(CoreError::Sqlx)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        scope_type: &str,
        peer_ip: Option<&str>,
        user_id: Option<Uuid>,
        reason: Option<&str>,
        active_until: Option<OffsetDateTime>,
        created_by: Uuid,
        action: &str,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, BanRule>(
            "INSERT INTO ban_rules \
               (id, scope_type, peer_ip, user_id, reason, active_until, created_by, action) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING *",
        )
        .bind(id)
        .bind(scope_type)
        .bind(peer_ip)
        .bind(user_id)
        .bind(reason)
        .bind(active_until)
        .bind(created_by)
        .bind(action)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn soft_delete(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE ban_rules SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn restore(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE ban_rules SET deleted_at = NULL WHERE id = $1 AND deleted_at IS NOT NULL",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
