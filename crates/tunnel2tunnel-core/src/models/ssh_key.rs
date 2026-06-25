use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;
use crate::error::CoreError;
use crate::timestamps::TimestampsSoftDelete;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct SshKey {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub algorithm: String,
    pub key_data: String,
    pub comment: Option<String>,
    pub fingerprint: String,
    pub name: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: TimestampsSoftDelete,
}

impl SshKey {
    pub async fn find_by_fingerprint(
        pool: &PgPool,
        fingerprint: &str,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, SshKey>(
            "SELECT * FROM ssh_keys WHERE fingerprint = $1 AND deleted_at IS NULL",
        )
        .bind(fingerprint)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, SshKey>(
            "SELECT sk.* FROM ssh_keys sk \
             JOIN entities e ON e.id = sk.entity_id \
             WHERE e.user_id = $1 AND sk.deleted_at IS NULL AND e.deleted_at IS NULL \
             ORDER BY sk.created_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn list_for_entity(pool: &PgPool, entity_id: Uuid) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, SshKey>(
            "SELECT * FROM ssh_keys WHERE entity_id = $1 AND deleted_at IS NULL ORDER BY created_at",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        entity_id: Uuid,
        algorithm: &str,
        key_data: &str,
        comment: Option<&str>,
        name: Option<&str>,
        valid_until: Option<OffsetDateTime>,
    ) -> Result<Self, CoreError> {
        let fingerprint = crate::pubkey::compute_fingerprint(key_data)?;
        let id = Uuid::now_v7();
        sqlx::query_as::<_, SshKey>(
            "INSERT INTO ssh_keys \
               (id, entity_id, algorithm, key_data, comment, fingerprint, name, valid_until) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING *",
        )
        .bind(id)
        .bind(entity_id)
        .bind(algorithm)
        .bind(key_data)
        .bind(comment)
        .bind(&fingerprint)
        .bind(name)
        .bind(valid_until)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn soft_delete(
        pool: &PgPool,
        id: Uuid,
        entity_id: Uuid,
    ) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE ssh_keys SET deleted_at = NOW() \
             WHERE id = $1 AND entity_id = $2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(entity_id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
