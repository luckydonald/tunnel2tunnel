use crate::error::CoreError;
use crate::timestamps::TimestampsSoftDelete;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Entity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: Option<String>,
    pub description: Option<String>,
    pub ip_whitelist: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: TimestampsSoftDelete,
}

impl Entity {
    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, Entity>(
            "SELECT * FROM entities \
             WHERE user_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn find_by_id_only(pool: &PgPool, id: Uuid) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Entity>("SELECT * FROM entities WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(CoreError::Sqlx)
    }

    pub async fn find_by_id_and_user(
        pool: &PgPool,
        id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Entity>(
            "SELECT * FROM entities WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        ip_whitelist: Option<&str>,
        valid_until: Option<OffsetDateTime>,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, Entity>(
            "INSERT INTO entities \
               (id, user_id, name, description, ip_whitelist, valid_until) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING *",
        )
        .bind(id)
        .bind(user_id)
        .bind(name)
        .bind(description)
        .bind(ip_whitelist)
        .bind(valid_until)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        user_id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        ip_whitelist: Option<&str>,
        valid_until: Option<OffsetDateTime>,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Entity>(
            "UPDATE entities \
             SET name = $3, description = $4, ip_whitelist = $5, valid_until = $6 \
             WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL \
             RETURNING *",
        )
        .bind(id)
        .bind(user_id)
        .bind(name)
        .bind(description)
        .bind(ip_whitelist)
        .bind(valid_until)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn soft_delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE entities SET deleted_at = NOW() \
             WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
