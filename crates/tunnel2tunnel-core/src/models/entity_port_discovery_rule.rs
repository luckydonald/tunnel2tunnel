use std::collections::HashMap;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CoreError;
use crate::models::entity::Entity;
use crate::models::entity_port::EntityPort;
use crate::timestamps::{SoftDelete, Timestamps, TimestampsSoftDelete};

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EntityPortDiscoveryRule {
    pub id: Uuid,
    pub client_entity_id: Uuid,
    pub server_port_id: Uuid,
    pub state: String,
    pub client_port_id: Option<Uuid>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

/// A server entity's enabled port with its discovery state relative to a specific client.
#[derive(Debug)]
pub struct DiscoveredPort {
    pub port: EntityPort,
    /// `None` = auto (no DB record); `Some("enabled")` or `Some("disabled")`.
    pub discovery_state: Option<String>,
    /// Set when `discovery_state == Some("enabled")`: the linked client EntityPort id.
    pub client_port_id: Option<Uuid>,
}

/// A server entity reachable by a specific client, with its discovered ports.
#[derive(Debug)]
pub struct ReachableServer {
    pub entity: Entity,
    /// Best-matching hostname alias from entity_access for the querying client.
    pub hostname: Option<String>,
    pub ports: Vec<DiscoveredPort>,
}

// ── Internal flat row types for SQLx queries ──────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
struct ReachableEntityRow {
    id: Uuid,
    user_id: Uuid,
    entity_type: String,
    name: Option<String>,
    description: Option<String>,
    ip_whitelist: Option<String>,
    valid_until: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    deleted_at: Option<OffsetDateTime>,
    hostname: Option<String>,
}

impl ReachableEntityRow {
    fn into_parts(self) -> (Entity, Option<String>) {
        let hostname = self.hostname;
        let entity = Entity {
            id: self.id,
            user_id: self.user_id,
            entity_type: self.entity_type,
            name: self.name,
            description: self.description,
            ip_whitelist: self.ip_whitelist,
            valid_until: self.valid_until,
            ts: TimestampsSoftDelete {
                timestamps: Timestamps { created_at: self.created_at, updated_at: self.updated_at },
                soft_delete: SoftDelete { deleted_at: self.deleted_at },
            },
        };
        (entity, hostname)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct DiscoveryPortRow {
    id: Uuid,
    entity_id: Uuid,
    enabled: bool,
    local_port: i32,
    proxy_port: i32,
    name: Option<String>,
    description: Option<String>,
    sort_order: i32,
    host: String,
    server_entity_id: Option<Uuid>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    discovery_state: Option<String>,
    client_port_id: Option<Uuid>,
}

impl DiscoveryPortRow {
    fn into_discovered_port(self) -> DiscoveredPort {
        let port = EntityPort {
            id: self.id,
            entity_id: self.entity_id,
            enabled: self.enabled,
            local_port: self.local_port,
            proxy_port: self.proxy_port,
            name: self.name,
            description: self.description,
            sort_order: self.sort_order,
            host: self.host,
            server_entity_id: self.server_entity_id,
            ts: Timestamps { created_at: self.created_at, updated_at: self.updated_at },
        };
        DiscoveredPort {
            port,
            discovery_state: self.discovery_state,
            client_port_id: self.client_port_id,
        }
    }
}

// ── CRUD ──────────────────────────────────────────────────────────────────────

impl EntityPortDiscoveryRule {
    pub async fn find(
        pool: &PgPool,
        client_entity_id: Uuid,
        server_port_id: Uuid,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM entity_port_discovery_rules \
             WHERE client_entity_id = $1 AND server_port_id = $2",
        )
        .bind(client_entity_id)
        .bind(server_port_id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Insert or update a discovery rule.
    pub async fn upsert(
        pool: &PgPool,
        client_entity_id: Uuid,
        server_port_id: Uuid,
        state: &str,
        client_port_id: Option<Uuid>,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, Self>(
            "INSERT INTO entity_port_discovery_rules \
               (id, client_entity_id, server_port_id, state, client_port_id) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (client_entity_id, server_port_id) DO UPDATE \
               SET state = EXCLUDED.state, client_port_id = EXCLUDED.client_port_id \
             RETURNING *",
        )
        .bind(id)
        .bind(client_entity_id)
        .bind(server_port_id)
        .bind(state)
        .bind(client_port_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Delete a discovery rule and return the deleted row (if any).
    pub async fn delete(
        pool: &PgPool,
        client_entity_id: Uuid,
        server_port_id: Uuid,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Self>(
            "DELETE FROM entity_port_discovery_rules \
             WHERE client_entity_id = $1 AND server_port_id = $2 \
             RETURNING *",
        )
        .bind(client_entity_id)
        .bind(server_port_id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Return server entities (with their enabled ports and discovery state) that
    /// the given client entity is authorized to reach via any entity_access rule.
    pub async fn list_reachable_for_client(
        pool: &PgPool,
        client_entity_id: Uuid,
        client_user_id: Uuid,
    ) -> Result<Vec<ReachableServer>, CoreError> {
        let entity_rows = sqlx::query_as::<_, ReachableEntityRow>(
            "SELECT DISTINCT ON (e.id) \
                e.id, e.user_id, e.type AS entity_type, e.name, e.description, \
                e.ip_whitelist, e.valid_until, e.created_at, e.updated_at, e.deleted_at, \
                ( \
                  SELECT ea2.hostname FROM entity_access ea2 \
                  WHERE ea2.owner_entity_id = e.id \
                    AND ea2.hostname IS NOT NULL \
                    AND ( \
                      (ea2.subject_type = 'entity'            AND ea2.subject_entity_id = $1) \
                      OR (ea2.subject_type = 'all_user_entities' AND ea2.subject_user_id  = $2) \
                      OR (ea2.subject_type = 'all_mine'          AND e.user_id            = $2) \
                      OR  ea2.subject_type = 'public_lite' \
                    ) \
                  ORDER BY (ea2.subject_type = 'entity' AND ea2.subject_entity_id = $1) DESC \
                  LIMIT 1 \
                ) AS hostname \
             FROM entities e \
             JOIN entity_access ea ON ea.owner_entity_id = e.id \
             WHERE e.type = 'server' \
               AND e.deleted_at IS NULL \
               AND ( \
                 ea.subject_type = 'public_lite' \
                 OR (ea.subject_type = 'all_mine'          AND e.user_id         = $2) \
                 OR (ea.subject_type = 'all_user_entities' AND ea.subject_user_id = $2) \
                 OR (ea.subject_type = 'entity'            AND ea.subject_entity_id = $1) \
               ) \
             ORDER BY e.id",
        )
        .bind(client_entity_id)
        .bind(client_user_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        if entity_rows.is_empty() {
            return Ok(vec![]);
        }

        let server_ids: Vec<Uuid> = entity_rows.iter().map(|r| r.id).collect();

        let port_rows = sqlx::query_as::<_, DiscoveryPortRow>(
            "SELECT ep.id, ep.entity_id, ep.enabled, ep.local_port, ep.proxy_port, \
                    ep.name, ep.description, ep.sort_order, ep.server_entity_id, \
                    ep.created_at, ep.updated_at, \
                    epdr.state AS discovery_state, epdr.client_port_id \
             FROM entity_ports ep \
             LEFT JOIN entity_port_discovery_rules epdr \
                 ON epdr.server_port_id = ep.id AND epdr.client_entity_id = $2 \
             WHERE ep.entity_id = ANY($1) AND ep.enabled = true \
             ORDER BY ep.entity_id, ep.sort_order, ep.created_at",
        )
        .bind(&server_ids[..])
        .bind(client_entity_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        let mut ports_by_entity: HashMap<Uuid, Vec<DiscoveredPort>> = HashMap::new();
        for row in port_rows {
            let eid = row.entity_id;
            ports_by_entity.entry(eid).or_default().push(row.into_discovered_port());
        }

        let result = entity_rows
            .into_iter()
            .map(|row| {
                let eid = row.id;
                let (entity, hostname) = row.into_parts();
                ReachableServer {
                    entity,
                    hostname,
                    ports: ports_by_entity.remove(&eid).unwrap_or_default(),
                }
            })
            .collect();

        Ok(result)
    }
}
