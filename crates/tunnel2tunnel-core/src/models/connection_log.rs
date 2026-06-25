use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectionLog {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub login_succeeded: bool,
    pub failure_reason: Option<String>,
    pub ssh_flags: Option<String>,
    pub ports_requested: Option<String>,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    pub ts: Timestamps,
}

impl ConnectionLog {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        entity_id: Option<Uuid>,
        peer_ip: Option<&str>,
        key_fingerprint: Option<&str>,
        login_succeeded: bool,
        failure_reason: Option<&str>,
        ssh_flags: Option<&str>,
        started_at: OffsetDateTime,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, ConnectionLog>(
            "INSERT INTO connection_logs \
               (id, entity_id, peer_ip, key_fingerprint, login_succeeded, \
                failure_reason, ssh_flags, started_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING *",
        )
        .bind(id)
        .bind(entity_id)
        .bind(peer_ip)
        .bind(key_fingerprint)
        .bind(login_succeeded)
        .bind(failure_reason)
        .bind(ssh_flags)
        .bind(started_at)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn set_ended(pool: &PgPool, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE connection_logs SET ended_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(CoreError::Sqlx)?;
        Ok(())
    }

    pub async fn list_for_entity(
        pool: &PgPool,
        entity_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, ConnectionLog>(
            "SELECT * FROM connection_logs \
             WHERE entity_id = $1 ORDER BY started_at DESC LIMIT $2",
        )
        .bind(entity_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }
}
