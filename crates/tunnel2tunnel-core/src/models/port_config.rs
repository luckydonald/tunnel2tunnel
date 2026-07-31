use crate::error::CoreError;
use crate::timestamps::Timestamps;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// The single declaration of "entity X offers this port as a service."
/// Owned by whichever entity declares it — an entity offering a service is
/// not a separate "server" role, it's just any entity with at least one of
/// these rows (see `is_server`/`is_client` computed booleans on `Entity`
/// responses).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct PortConfig {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i32,
    /// The host to forward connections to (default: "localhost"). Lets you
    /// expose a service on another machine reachable from the owning entity.
    pub host: String,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

impl PortConfig {
    pub async fn list_for_entity(pool: &PgPool, entity_id: Uuid) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, PortConfig>(
            "SELECT * FROM port_configs WHERE entity_id = $1 ORDER BY sort_order, created_at",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        entity_id: Uuid,
        enabled: bool,
        local_port: i32,
        proxy_port: i32,
        name: &str,
        description: Option<&str>,
        sort_order: i32,
        host: &str,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, PortConfig>(
            "INSERT INTO port_configs \
               (id, entity_id, enabled, local_port, proxy_port, name, description, sort_order, host) \
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
        .bind(host)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        entity_id: Uuid,
        enabled: bool,
        local_port: i32,
        proxy_port: i32,
        name: &str,
        description: Option<&str>,
        sort_order: i32,
        host: &str,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, PortConfig>(
            "UPDATE port_configs \
             SET enabled=$3, local_port=$4, proxy_port=$5, name=$6, description=$7, \
                 sort_order=$8, host=$9 \
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
        .bind(host)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, PortConfig>("SELECT * FROM port_configs WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(CoreError::Sqlx)
    }

    /// Find the enabled port_config for `(entity_id, proxy_port)` — used by
    /// the SSH layer to authorize `-L` connections and to check whether a
    /// `-R` registration needs to auto-create a new row.
    pub async fn find_enabled_by_entity_and_proxy_port(
        pool: &PgPool,
        entity_id: Uuid,
        proxy_port: i32,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, PortConfig>(
            "SELECT * FROM port_configs \
             WHERE entity_id = $1 AND proxy_port = $2 AND enabled = true \
             ORDER BY created_at LIMIT 1",
        )
        .bind(entity_id)
        .bind(proxy_port)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn delete(pool: &PgPool, id: Uuid, entity_id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query("DELETE FROM port_configs WHERE id = $1 AND entity_id = $2")
            .bind(id)
            .bind(entity_id)
            .execute(pool)
            .await
            .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    /// True iff `entity_id` owns at least one `port_configs` row — the
    /// computed `is_server` badge.
    pub async fn entity_has_any(pool: &PgPool, entity_id: Uuid) -> Result<bool, CoreError> {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM port_configs WHERE entity_id = $1)",
        )
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Batched version of `entity_has_any` for listing many entities without
    /// N+1 queries — mirrors `ConnectionLog::entity_statuses`.
    pub async fn entities_have_any(
        pool: &PgPool,
        entity_ids: &[Uuid],
    ) -> Result<std::collections::HashSet<Uuid>, CoreError> {
        if entity_ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        let ids: Vec<Uuid> = sqlx::query_scalar::<_, Uuid>(
            "SELECT DISTINCT entity_id FROM port_configs WHERE entity_id = ANY($1)",
        )
        .bind(entity_ids)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(ids.into_iter().collect())
    }

    /// Every `port_configs` row across all entities, joined with the owning
    /// entity/user — used for the admin live-connections dashboard's
    /// "server-role" rows.
    pub async fn list_all_with_owner(pool: &PgPool) -> Result<Vec<PortConfigWithOwner>, CoreError> {
        let rows = sqlx::query_as::<_, PortConfigOwnerRow>(
            "SELECT pc.id, pc.entity_id, pc.enabled, pc.local_port, pc.proxy_port, pc.name, \
                    pc.description, pc.sort_order, pc.host, pc.created_at, pc.updated_at, \
                    e.name AS owner_entity_name, e.user_id AS owner_user_id, \
                    u.username AS owner_username \
             FROM port_configs pc \
             JOIN entities e ON e.id = pc.entity_id AND e.deleted_at IS NULL \
             JOIN users u ON u.id = e.user_id \
             ORDER BY pc.entity_id, pc.sort_order, pc.created_at",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        Ok(rows
            .into_iter()
            .map(|r| PortConfigWithOwner {
                port_config: PortConfig {
                    id: r.id,
                    entity_id: r.entity_id,
                    enabled: r.enabled,
                    local_port: r.local_port,
                    proxy_port: r.proxy_port,
                    name: r.name,
                    description: r.description,
                    sort_order: r.sort_order,
                    host: r.host,
                    ts: Timestamps {
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    },
                },
                owner_entity_name: r.owner_entity_name,
                owner_user_id: r.owner_user_id,
                owner_username: r.owner_username,
            })
            .collect())
    }
}

/// A `port_configs` row joined with its owning entity/user — see
/// `PortConfig::list_all_with_owner`.
#[derive(Debug, Clone)]
pub struct PortConfigWithOwner {
    pub port_config: PortConfig,
    pub owner_entity_name: Option<String>,
    pub owner_user_id: Uuid,
    pub owner_username: String,
}

#[derive(Debug, sqlx::FromRow)]
struct PortConfigOwnerRow {
    id: Uuid,
    entity_id: Uuid,
    enabled: bool,
    local_port: i32,
    proxy_port: i32,
    name: String,
    description: Option<String>,
    sort_order: i32,
    host: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    owner_entity_name: Option<String>,
    owner_user_id: Uuid,
    owner_username: String,
}
