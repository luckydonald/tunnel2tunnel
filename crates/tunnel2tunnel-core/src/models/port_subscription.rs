use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CoreError;
use crate::models::entity::Entity;
use crate::models::port_config::PortConfig;
use crate::timestamps::{SoftDelete, Timestamps, TimestampsSoftDelete};

/// The M2M: "entity B wants port_config P routed to itself, on this local
/// port." Replaces `entity_port_discovery_rules` entirely — a row's mere
/// existence + `enabled` is the whole state (no row = not subscribed at
/// all; `enabled=false` = paused without losing the chosen local port).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct PortSubscription {
    pub id: Uuid,
    pub port_config_id: Uuid,
    pub subscriber_entity_id: Uuid,
    pub subscriber_local_port: i32,
    pub enabled: bool,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

/// A subscribable service owned by some other entity, alongside the
/// querying entity's own subscription row, if any.
#[derive(Debug)]
pub struct SubscribableService {
    pub port_config: PortConfig,
    pub subscription: Option<PortSubscription>,
}

/// A service-owning entity along with every one of its services the
/// querying entity is allowed to see/subscribe to.
#[derive(Debug)]
pub struct SubscribableOwner {
    pub entity: Entity,
    pub services: Vec<SubscribableService>,
}

// ── Internal flat row type for the browse query ───────────────────────────────

#[derive(Debug, sqlx::FromRow)]
struct SubscribableRow {
    // owner entity
    e_id: Uuid,
    e_user_id: Uuid,
    e_name: Option<String>,
    e_description: Option<String>,
    e_ip_whitelist: Option<String>,
    e_valid_until: Option<OffsetDateTime>,
    e_created_at: OffsetDateTime,
    e_updated_at: OffsetDateTime,
    e_deleted_at: Option<OffsetDateTime>,
    // port_config
    pc_id: Uuid,
    pc_entity_id: Uuid,
    pc_enabled: bool,
    pc_local_port: i32,
    pc_proxy_port: i32,
    pc_name: String,
    pc_description: Option<String>,
    pc_sort_order: i32,
    pc_host: String,
    pc_created_at: OffsetDateTime,
    pc_updated_at: OffsetDateTime,
    // subscriber's own subscription (nullable)
    ps_id: Option<Uuid>,
    ps_subscriber_local_port: Option<i32>,
    ps_enabled: Option<bool>,
    ps_created_at: Option<OffsetDateTime>,
    ps_updated_at: Option<OffsetDateTime>,
}

impl SubscribableRow {
    fn into_parts(self, subscriber_entity_id: Uuid) -> (Entity, SubscribableService) {
        let entity = Entity {
            id: self.e_id,
            user_id: self.e_user_id,
            name: self.e_name,
            description: self.e_description,
            ip_whitelist: self.e_ip_whitelist,
            valid_until: self.e_valid_until,
            ts: TimestampsSoftDelete {
                timestamps: Timestamps {
                    created_at: self.e_created_at,
                    updated_at: self.e_updated_at,
                },
                soft_delete: SoftDelete {
                    deleted_at: self.e_deleted_at,
                },
            },
        };
        let port_config = PortConfig {
            id: self.pc_id,
            entity_id: self.pc_entity_id,
            enabled: self.pc_enabled,
            local_port: self.pc_local_port,
            proxy_port: self.pc_proxy_port,
            name: self.pc_name,
            description: self.pc_description,
            sort_order: self.pc_sort_order,
            host: self.pc_host,
            ts: Timestamps {
                created_at: self.pc_created_at,
                updated_at: self.pc_updated_at,
            },
        };
        let subscription = self.ps_id.map(|id| PortSubscription {
            id,
            port_config_id: port_config.id,
            subscriber_entity_id,
            subscriber_local_port: self.ps_subscriber_local_port.unwrap_or_default(),
            enabled: self.ps_enabled.unwrap_or(false),
            ts: Timestamps {
                created_at: self.ps_created_at.unwrap_or(self.e_created_at),
                updated_at: self.ps_updated_at.unwrap_or(self.e_updated_at),
            },
        });
        (
            entity,
            SubscribableService {
                port_config,
                subscription,
            },
        )
    }
}

impl PortSubscription {
    pub async fn create(
        pool: &PgPool,
        port_config_id: Uuid,
        subscriber_entity_id: Uuid,
        subscriber_local_port: i32,
        enabled: bool,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, PortSubscription>(
            "INSERT INTO port_subscriptions \
               (id, port_config_id, subscriber_entity_id, subscriber_local_port, enabled) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING *",
        )
        .bind(id)
        .bind(port_config_id)
        .bind(subscriber_entity_id)
        .bind(subscriber_local_port)
        .bind(enabled)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Update `subscriber_local_port` and/or `enabled` on an existing
    /// subscription. `None` leaves the corresponding column unchanged.
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        subscriber_entity_id: Uuid,
        subscriber_local_port: Option<i32>,
        enabled: Option<bool>,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, PortSubscription>(
            "UPDATE port_subscriptions \
             SET subscriber_local_port = COALESCE($3, subscriber_local_port), \
                 enabled = COALESCE($4, enabled) \
             WHERE id = $1 AND subscriber_entity_id = $2 \
             RETURNING *",
        )
        .bind(id)
        .bind(subscriber_entity_id)
        .bind(subscriber_local_port)
        .bind(enabled)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn delete(
        pool: &PgPool,
        id: Uuid,
        subscriber_entity_id: Uuid,
    ) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "DELETE FROM port_subscriptions WHERE id = $1 AND subscriber_entity_id = $2",
        )
        .bind(id)
        .bind(subscriber_entity_id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn find_by_subscriber_and_port_config(
        pool: &PgPool,
        subscriber_entity_id: Uuid,
        port_config_id: Uuid,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, PortSubscription>(
            "SELECT * FROM port_subscriptions \
             WHERE subscriber_entity_id = $1 AND port_config_id = $2",
        )
        .bind(subscriber_entity_id)
        .bind(port_config_id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// True iff `entity_id` owns at least one `port_subscriptions` row — the
    /// computed `is_client` badge.
    pub async fn entity_has_any(pool: &PgPool, entity_id: Uuid) -> Result<bool, CoreError> {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM port_subscriptions WHERE subscriber_entity_id = $1)",
        )
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Batched version of `entity_has_any` for listing many entities without
    /// N+1 queries.
    pub async fn entities_have_any(
        pool: &PgPool,
        entity_ids: &[Uuid],
    ) -> Result<std::collections::HashSet<Uuid>, CoreError> {
        if entity_ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        let ids: Vec<Uuid> = sqlx::query_scalar::<_, Uuid>(
            "SELECT DISTINCT subscriber_entity_id FROM port_subscriptions \
             WHERE subscriber_entity_id = ANY($1)",
        )
        .bind(entity_ids)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(ids.into_iter().collect())
    }

    /// Browse every service the given subscriber entity/user is authorized
    /// to reach, grouped by owning entity — replaces
    /// `EntityPortDiscoveryRule::list_reachable_for_client`. Unions:
    ///  (a) whole-entity `entity_access` grants (`port_config_id IS NULL`) —
    ///      all of that owner's enabled port_configs;
    ///  (b) port-scoped `entity_access` grants (`port_config_id` set) — just
    ///      that one port_config, even without a whole-entity grant;
    ///  (c) same-account bypass — any entity owned by `subscriber_user_id`
    ///      is always includable, regardless of `entity_access` rows.
    /// For each returned port_config, the subscriber's own `port_subscriptions`
    /// row is left-joined in so callers can show subscribed-vs-not.
    pub async fn list_subscribable_for_entity(
        pool: &PgPool,
        subscriber_entity_id: Uuid,
        subscriber_user_id: Uuid,
    ) -> Result<Vec<SubscribableOwner>, CoreError> {
        let rows = sqlx::query_as::<_, SubscribableRow>(
            "WITH visible_owner_entities AS ( \
                SELECT DISTINCT e.id AS owner_entity_id \
                FROM entities e \
                JOIN entity_access ea ON ea.owner_entity_id = e.id AND ea.port_config_id IS NULL \
                WHERE e.deleted_at IS NULL \
                  AND ( \
                    e.user_id = $2 \
                    OR ea.subject_type = 'public_lite' \
                    OR (ea.subject_type = 'all_user_entities' AND ea.subject_user_id = $2) \
                    OR (ea.subject_type = 'entity' AND ea.subject_entity_id = $1) \
                  ) \
                UNION \
                SELECT DISTINCT e.id AS owner_entity_id \
                FROM entities e \
                WHERE e.deleted_at IS NULL AND e.user_id = $2 AND e.id != $1 \
             ), \
             visible_services AS ( \
                SELECT pc.id AS port_config_id \
                FROM port_configs pc \
                JOIN visible_owner_entities voe ON voe.owner_entity_id = pc.entity_id \
                WHERE pc.enabled = true \
                UNION \
                SELECT pc.id AS port_config_id \
                FROM port_configs pc \
                JOIN entity_access ea ON ea.owner_entity_id = pc.entity_id AND ea.port_config_id = pc.id \
                JOIN entities e ON e.id = pc.entity_id \
                WHERE pc.enabled = true \
                  AND e.deleted_at IS NULL \
                  AND ( \
                    e.user_id = $2 \
                    OR ea.subject_type = 'public_lite' \
                    OR (ea.subject_type = 'all_user_entities' AND ea.subject_user_id = $2) \
                    OR (ea.subject_type = 'entity' AND ea.subject_entity_id = $1) \
                  ) \
             ) \
             SELECT \
                e.id AS e_id, e.user_id AS e_user_id, e.name AS e_name, \
                e.description AS e_description, e.ip_whitelist AS e_ip_whitelist, \
                e.valid_until AS e_valid_until, e.created_at AS e_created_at, \
                e.updated_at AS e_updated_at, e.deleted_at AS e_deleted_at, \
                pc.id AS pc_id, pc.entity_id AS pc_entity_id, pc.enabled AS pc_enabled, \
                pc.local_port AS pc_local_port, pc.proxy_port AS pc_proxy_port, \
                pc.name AS pc_name, pc.description AS pc_description, \
                pc.sort_order AS pc_sort_order, pc.host AS pc_host, \
                pc.created_at AS pc_created_at, pc.updated_at AS pc_updated_at, \
                ps.id AS ps_id, ps.subscriber_local_port AS ps_subscriber_local_port, \
                ps.enabled AS ps_enabled, ps.created_at AS ps_created_at, \
                ps.updated_at AS ps_updated_at \
             FROM visible_services vs \
             JOIN port_configs pc ON pc.id = vs.port_config_id \
             JOIN entities e ON e.id = pc.entity_id \
             LEFT JOIN port_subscriptions ps \
               ON ps.port_config_id = pc.id AND ps.subscriber_entity_id = $1 \
             WHERE pc.entity_id != $1 \
             ORDER BY e.id, pc.sort_order, pc.created_at",
        )
        .bind(subscriber_entity_id)
        .bind(subscriber_user_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        let mut order: Vec<Uuid> = Vec::new();
        let mut by_owner: HashMap<Uuid, (Entity, Vec<SubscribableService>)> = HashMap::new();
        for row in rows {
            let owner_id = row.e_id;
            let (entity, service) = row.into_parts(subscriber_entity_id);
            by_owner
                .entry(owner_id)
                .or_insert_with(|| {
                    order.push(owner_id);
                    (entity, Vec::new())
                })
                .1
                .push(service);
        }

        Ok(order
            .into_iter()
            .filter_map(|id| by_owner.remove(&id))
            .map(|(entity, services)| SubscribableOwner { entity, services })
            .collect())
    }
}
