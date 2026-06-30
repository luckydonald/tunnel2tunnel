use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EntityAccess {
    pub id: Uuid,
    pub owner_entity_id: Uuid,
    pub subject_type: String,
    pub subject_entity_id: Option<Uuid>,
    pub subject_user_id: Option<Uuid>,
    pub hostname: Option<String>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

impl EntityAccess {
    pub async fn list_for_user(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, EntityAccess>(
            "SELECT ea.* FROM entity_access ea \
             JOIN entities e ON e.id = ea.owner_entity_id \
             WHERE e.user_id = $1 AND e.deleted_at IS NULL \
             ORDER BY ea.created_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn list_for_entity(
        pool: &PgPool,
        owner_entity_id: Uuid,
    ) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, EntityAccess>(
            "SELECT * FROM entity_access \
             WHERE owner_entity_id = $1 ORDER BY created_at DESC",
        )
        .bind(owner_entity_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        owner_entity_id: Uuid,
        subject_type: &str,
        subject_entity_id: Option<Uuid>,
        subject_user_id: Option<Uuid>,
        hostname: Option<&str>,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, EntityAccess>(
            "INSERT INTO entity_access \
               (id, owner_entity_id, subject_type, subject_entity_id, subject_user_id, hostname) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING *",
        )
        .bind(id)
        .bind(owner_entity_id)
        .bind(subject_type)
        .bind(subject_entity_id)
        .bind(subject_user_id)
        .bind(hostname)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn delete(
        pool: &PgPool,
        id: Uuid,
        owner_entity_id: Uuid,
    ) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "DELETE FROM entity_access WHERE id = $1 AND owner_entity_id = $2",
        )
        .bind(id)
        .bind(owner_entity_id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    /// Returns true if `client_entity_id` (owned by `client_user_id`) has an
    /// access rule on `target_entity_id` (owned by `target_user_id`).
    pub async fn check_access(
        pool: &PgPool,
        target_entity_id: Uuid,
        client_entity_id: Uuid,
        client_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, CoreError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
               SELECT 1 FROM entity_access
               WHERE owner_entity_id = $1
                 AND (
                   subject_type = 'public_lite'
                   OR (subject_type = 'all_mine'            AND $3 = $4)
                   OR (subject_type = 'all_user_entities'   AND subject_user_id = $3)
                   OR (subject_type = 'entity'              AND subject_entity_id = $2)
                 )
             )",
        )
        .bind(target_entity_id)
        .bind(client_entity_id)
        .bind(client_user_id)
        .bind(target_user_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(exists)
    }

    pub async fn list_incoming(
        pool: &PgPool,
        subject_entity_id: Uuid,
    ) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, EntityAccess>(
            "SELECT * FROM entity_access \
             WHERE subject_type = 'entity' AND subject_entity_id = $1 \
             ORDER BY created_at DESC",
        )
        .bind(subject_entity_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Resolve a hostname alias to an entity ID.
    pub async fn find_entity_by_hostname(
        pool: &PgPool,
        hostname: &str,
    ) -> Result<Option<Uuid>, CoreError> {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT owner_entity_id FROM entity_access \
             WHERE hostname = $1 LIMIT 1",
        )
        .bind(hostname)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }
}
