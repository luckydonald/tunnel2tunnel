use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EntityPort {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub sort_order: i32,
    /// For client-side ports: the server entity this tunnel routes through.
    /// `None` for server-side ports or pre-migration ports.
    pub server_entity_id: Option<Uuid>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

impl EntityPort {
    pub async fn list_for_entity(pool: &PgPool, entity_id: Uuid) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, EntityPort>(
            "SELECT * FROM entity_ports WHERE entity_id = $1 ORDER BY sort_order, created_at",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        entity_id: Uuid,
        enabled: bool,
        local_port: i32,
        proxy_port: i32,
        name: Option<&str>,
        description: Option<&str>,
        sort_order: i32,
        server_entity_id: Option<Uuid>,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, EntityPort>(
            "INSERT INTO entity_ports \
               (id, entity_id, enabled, local_port, proxy_port, name, description, sort_order, server_entity_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             RETURNING *",
        )
        .bind(id)
        .bind(entity_id)
        .bind(enabled)
        .bind(local_port)
        .bind(proxy_port)
        .bind(name)
        .bind(description)
        .bind(sort_order)
        .bind(server_entity_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        entity_id: Uuid,
        enabled: bool,
        local_port: i32,
        proxy_port: i32,
        name: Option<&str>,
        description: Option<&str>,
        sort_order: i32,
        server_entity_id: Option<Uuid>,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, EntityPort>(
            "UPDATE entity_ports \
             SET enabled=$3, local_port=$4, proxy_port=$5, name=$6, description=$7, \
                 sort_order=$8, server_entity_id=$9 \
             WHERE id = $1 AND entity_id = $2 \
             RETURNING *",
        )
        .bind(id)
        .bind(entity_id)
        .bind(enabled)
        .bind(local_port)
        .bind(proxy_port)
        .bind(name)
        .bind(description)
        .bind(sort_order)
        .bind(server_entity_id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, EntityPort>(
            "SELECT * FROM entity_ports WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn delete(pool: &PgPool, id: Uuid, entity_id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "DELETE FROM entity_ports WHERE id = $1 AND entity_id = $2",
        )
        .bind(id)
        .bind(entity_id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
