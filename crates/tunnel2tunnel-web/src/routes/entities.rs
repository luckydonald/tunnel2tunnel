use crate::{extractors::AuthUser, AppState, WebError};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tunnel2tunnel_core::models::{
    connection_log::ConnectionLog,
    entity::Entity,
    entity_access::EntityAccess,
    port_config::PortConfig,
    port_subscription::{PortSubscription, SubscribableOwner, SubscribableService},
    ssh_key::SshKey,
};
use uuid::Uuid;

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EntityListQuery {
    /// Optional `server`/`client` filter over the computed `is_server`/
    /// `is_client` booleans — `type` is no longer a stored column.
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateEntityBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub ip_whitelist: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
}

#[derive(Deserialize)]
pub struct UpdateEntityBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub ip_whitelist: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
}

#[derive(Deserialize)]
pub struct AddKeyBody {
    pub algorithm: String,
    pub key_data: String,
    pub comment: Option<String>,
    pub name: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
}

#[derive(Deserialize)]
pub struct CreatePortBody {
    pub enabled: Option<bool>,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
    pub host: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePortBody {
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub host: Option<String>,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct EntityResponse {
    pub id: Uuid,
    pub name: Option<String>,
    pub description: Option<String>,
    pub ip_whitelist: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
    /// True iff there's currently an open, successful SSH session for this
    /// entity — derived from connection_logs, not a dedicated column.
    pub online: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_disconnected_at: Option<OffsetDateTime>,
    /// Computed: this entity owns at least one `port_configs` row.
    pub is_server: bool,
    /// Computed: this entity owns at least one `port_subscriptions` row.
    pub is_client: bool,
}

impl From<Entity> for EntityResponse {
    fn from(e: Entity) -> Self {
        EntityResponse {
            id: e.id,
            name: e.name,
            description: e.description,
            ip_whitelist: e.ip_whitelist,
            valid_until: e.valid_until,
            created_at: e.ts.timestamps.created_at,
            updated_at: e.ts.timestamps.updated_at,
            deleted_at: e.ts.soft_delete.deleted_at,
            online: false,
            last_disconnected_at: None,
            is_server: false,
            is_client: false,
        }
    }
}

impl EntityResponse {
    fn with_status(mut self, status: Option<(bool, Option<OffsetDateTime>)>) -> Self {
        if let Some((online, last_disconnected_at)) = status {
            self.online = online;
            self.last_disconnected_at = last_disconnected_at;
        }
        self
    }

    fn with_roles(mut self, is_server: bool, is_client: bool) -> Self {
        self.is_server = is_server;
        self.is_client = is_client;
        self
    }
}

#[derive(Serialize)]
pub struct SshKeyResponse {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub algorithm: String,
    pub key_data: String,
    pub comment: Option<String>,
    pub fingerprint: String,
    pub name: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub valid_until: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
}

impl From<SshKey> for SshKeyResponse {
    fn from(k: SshKey) -> Self {
        SshKeyResponse {
            id: k.id,
            entity_id: k.entity_id,
            algorithm: k.algorithm,
            key_data: k.key_data,
            comment: k.comment,
            fingerprint: k.fingerprint,
            name: k.name,
            valid_until: k.valid_until,
            created_at: k.ts.timestamps.created_at,
            updated_at: k.ts.timestamps.updated_at,
            deleted_at: k.ts.soft_delete.deleted_at,
        }
    }
}

#[derive(Serialize)]
pub struct PortConfigResponse {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub host: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl From<PortConfig> for PortConfigResponse {
    fn from(p: PortConfig) -> Self {
        PortConfigResponse {
            id: p.id,
            entity_id: p.entity_id,
            enabled: p.enabled,
            local_port: p.local_port,
            proxy_port: p.proxy_port,
            name: p.name,
            description: p.description,
            sort_order: p.sort_order,
            host: p.host,
            created_at: p.ts.created_at,
            updated_at: p.ts.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct EntityDetailResponse {
    #[serde(flatten)]
    pub entity: EntityResponse,
    pub ssh_keys: Vec<SshKeyResponse>,
    pub ports: Vec<PortConfigResponse>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) async fn require_owner(
    db: &sqlx::PgPool,
    entity_id: Uuid,
    user_id: Uuid,
) -> Result<Entity, WebError> {
    Entity::find_by_id_and_user(db, entity_id, user_id)
        .await?
        .ok_or(WebError::NotFound)
}

async fn entity_roles(db: &sqlx::PgPool, entity_id: Uuid) -> Result<(bool, bool), WebError> {
    let is_server = PortConfig::entity_has_any(db, entity_id)
        .await
        .map_err(WebError::Core)?;
    let is_client = PortSubscription::entity_has_any(db, entity_id)
        .await
        .map_err(WebError::Core)?;
    Ok((is_server, is_client))
}

// ── Handlers — entities ───────────────────────────────────────────────────────

pub async fn list_entities(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<EntityListQuery>,
) -> Result<Json<Vec<EntityResponse>>, WebError> {
    let list = Entity::list_for_user(&state.db, user.id).await?;
    let ids: Vec<Uuid> = list.iter().map(|e| e.id).collect();
    let statuses = ConnectionLog::entity_statuses(&state.db, &ids)
        .await
        .map_err(WebError::Core)?;
    let servers = PortConfig::entities_have_any(&state.db, &ids)
        .await
        .map_err(WebError::Core)?;
    let clients = PortSubscription::entities_have_any(&state.db, &ids)
        .await
        .map_err(WebError::Core)?;

    let mut result: Vec<EntityResponse> = list
        .into_iter()
        .map(|e| {
            let status = statuses.get(&e.id).copied();
            let is_server = servers.contains(&e.id);
            let is_client = clients.contains(&e.id);
            EntityResponse::from(e)
                .with_status(status)
                .with_roles(is_server, is_client)
        })
        .collect();

    if let Some(role) = q.role.as_deref() {
        result.retain(|e| match role {
            "server" => e.is_server,
            "client" => e.is_client,
            "unassigned" => !e.is_server && !e.is_client,
            _ => true,
        });
    }

    Ok(Json(result))
}

pub async fn create_entity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(b): Json<CreateEntityBody>,
) -> Result<(StatusCode, Json<EntityResponse>), WebError> {
    let e = Entity::create(
        &state.db,
        user.id,
        b.name.as_deref(),
        b.description.as_deref(),
        b.ip_whitelist.as_deref(),
        b.valid_until,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(EntityResponse::from(e))))
}

pub async fn get_entity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<EntityDetailResponse>, WebError> {
    let entity = require_owner(&state.db, id, user.id).await?;
    let ssh_keys = SshKey::list_for_entity(&state.db, id).await?;
    let ports = PortConfig::list_for_entity(&state.db, id).await?;
    let status = ConnectionLog::entity_status(&state.db, id)
        .await
        .map_err(WebError::Core)?;
    let (is_server, is_client) = entity_roles(&state.db, id).await?;
    Ok(Json(EntityDetailResponse {
        entity: EntityResponse::from(entity)
            .with_status(Some(status))
            .with_roles(is_server, is_client),
        ssh_keys: ssh_keys.into_iter().map(SshKeyResponse::from).collect(),
        ports: ports.into_iter().map(PortConfigResponse::from).collect(),
    }))
}

pub async fn update_entity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateEntityBody>,
) -> Result<Json<EntityResponse>, WebError> {
    let e = Entity::update(
        &state.db,
        id,
        user.id,
        b.name.as_deref(),
        b.description.as_deref(),
        b.ip_whitelist.as_deref(),
        b.valid_until,
    )
    .await?
    .ok_or(WebError::NotFound)?;
    let (is_server, is_client) = entity_roles(&state.db, id).await?;
    Ok(Json(EntityResponse::from(e).with_roles(is_server, is_client)))
}

pub async fn delete_entity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    if Entity::soft_delete(&state.db, id, user.id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Handlers — SSH keys ───────────────────────────────────────────────────────

pub async fn add_key(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
    Json(b): Json<AddKeyBody>,
) -> Result<(StatusCode, Json<SshKeyResponse>), WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let key = SshKey::create(
        &state.db,
        entity_id,
        &b.algorithm,
        &b.key_data,
        b.comment.as_deref(),
        b.name.as_deref(),
        b.valid_until,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(SshKeyResponse::from(key))))
}

#[derive(Deserialize)]
pub struct UpdateKeyBody {
    pub name: Option<String>,
    pub comment: Option<String>,
}

pub async fn update_key(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, key_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<UpdateKeyBody>,
) -> Result<Json<SshKeyResponse>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let key = SshKey::update(&state.db, key_id, entity_id, b.name.as_deref(), b.comment.as_deref())
        .await?
        .ok_or(WebError::NotFound)?;
    Ok(Json(SshKeyResponse::from(key)))
}

pub async fn delete_key(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    if SshKey::soft_delete(&state.db, key_id, entity_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Handlers — port_configs ("my services") ───────────────────────────────────

pub async fn list_ports(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<PortConfigResponse>>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let ports = PortConfig::list_for_entity(&state.db, entity_id).await?;
    Ok(Json(
        ports.into_iter().map(PortConfigResponse::from).collect(),
    ))
}

pub async fn create_port(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
    Json(b): Json<CreatePortBody>,
) -> Result<(StatusCode, Json<PortConfigResponse>), WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    if b.name.trim().is_empty() {
        return Err(WebError::BadRequest("name must not be empty".into()));
    }
    let port = PortConfig::create(
        &state.db,
        entity_id,
        b.enabled.unwrap_or(true),
        b.local_port,
        b.proxy_port,
        &b.name,
        b.description.as_deref(),
        b.sort_order.unwrap_or(0),
        b.host.as_deref().unwrap_or("localhost"),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(PortConfigResponse::from(port))))
}

pub async fn update_port(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, port_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<UpdatePortBody>,
) -> Result<Json<PortConfigResponse>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    if b.name.trim().is_empty() {
        return Err(WebError::BadRequest("name must not be empty".into()));
    }
    let port = PortConfig::update(
        &state.db,
        port_id,
        entity_id,
        b.enabled,
        b.local_port,
        b.proxy_port,
        &b.name,
        b.description.as_deref(),
        b.sort_order,
        b.host.as_deref().unwrap_or("localhost"),
    )
    .await?
    .ok_or(WebError::NotFound)?;
    Ok(Json(PortConfigResponse::from(port)))
}

pub async fn delete_port(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, port_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    if PortConfig::delete(&state.db, port_id, entity_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Handlers — port_subscriptions ("my subscriptions") ────────────────────────

#[derive(Serialize)]
pub struct PortSubscriptionResponse {
    pub id: Uuid,
    pub port_config_id: Uuid,
    pub subscriber_entity_id: Uuid,
    pub subscriber_local_port: i32,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl From<PortSubscription> for PortSubscriptionResponse {
    fn from(s: PortSubscription) -> Self {
        PortSubscriptionResponse {
            id: s.id,
            port_config_id: s.port_config_id,
            subscriber_entity_id: s.subscriber_entity_id,
            subscriber_local_port: s.subscriber_local_port,
            enabled: s.enabled,
            created_at: s.ts.created_at,
            updated_at: s.ts.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct SubscribableServiceResponse {
    #[serde(flatten)]
    pub port_config: PortConfigResponse,
    pub subscription: Option<PortSubscriptionResponse>,
}

impl From<SubscribableService> for SubscribableServiceResponse {
    fn from(s: SubscribableService) -> Self {
        SubscribableServiceResponse {
            port_config: PortConfigResponse::from(s.port_config),
            subscription: s.subscription.map(PortSubscriptionResponse::from),
        }
    }
}

#[derive(Serialize)]
pub struct SubscribableOwnerResponse {
    #[serde(flatten)]
    pub entity: EntityResponse,
    pub services: Vec<SubscribableServiceResponse>,
}

impl From<SubscribableOwner> for SubscribableOwnerResponse {
    fn from(o: SubscribableOwner) -> Self {
        SubscribableOwnerResponse {
            entity: EntityResponse::from(o.entity),
            services: o.services.into_iter().map(Into::into).collect(),
        }
    }
}

pub async fn list_subscribable_services(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<SubscribableOwnerResponse>>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let owners =
        PortSubscription::list_subscribable_for_entity(&state.db, entity_id, user.id).await?;
    Ok(Json(owners.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize)]
pub struct CreateSubscriptionBody {
    pub port_config_id: Uuid,
    pub subscriber_local_port: i32,
    pub enabled: Option<bool>,
}

pub async fn create_subscription(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
    Json(b): Json<CreateSubscriptionBody>,
) -> Result<(StatusCode, Json<PortSubscriptionResponse>), WebError> {
    require_owner(&state.db, entity_id, user.id).await?;

    let port_config = PortConfig::find_by_id(&state.db, b.port_config_id)
        .await?
        .ok_or(WebError::NotFound)?;
    let owner_entity = Entity::find_by_id_only(&state.db, port_config.entity_id)
        .await?
        .ok_or(WebError::NotFound)?;

    let allowed = EntityAccess::check_access(
        &state.db,
        owner_entity.id,
        Some(port_config.id),
        entity_id,
        user.id,
        owner_entity.user_id,
    )
    .await
    .map_err(WebError::Core)?;
    if !allowed {
        return Err(WebError::Forbidden);
    }

    let sub = PortSubscription::create(
        &state.db,
        port_config.id,
        entity_id,
        b.subscriber_local_port,
        b.enabled.unwrap_or(true),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(PortSubscriptionResponse::from(sub))))
}

#[derive(Deserialize)]
pub struct UpdateSubscriptionBody {
    pub subscriber_local_port: Option<i32>,
    pub enabled: Option<bool>,
}

pub async fn update_subscription(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, subscription_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<UpdateSubscriptionBody>,
) -> Result<Json<PortSubscriptionResponse>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let sub = PortSubscription::update(
        &state.db,
        subscription_id,
        entity_id,
        b.subscriber_local_port,
        b.enabled,
    )
    .await?
    .ok_or(WebError::NotFound)?;
    Ok(Json(PortSubscriptionResponse::from(sub)))
}

pub async fn delete_subscription(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, subscription_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    if PortSubscription::delete(&state.db, subscription_id, entity_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Connection log ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConnLogResponse {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub attempted_password: Option<String>,
    pub attempted_username: Option<String>,
    pub fail_reason: Option<String>,
    pub success_reason: Option<String>,
    pub success: bool,
    pub tarpit_method: Option<String>,
    pub tarpit_action: Option<String>,
    pub tarpit_threshold_id: Option<Uuid>,
    pub banned_by_ban_rule_id: Option<Uuid>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl From<tunnel2tunnel_core::models::connection_log::ConnectionLog> for ConnLogResponse {
    fn from(l: tunnel2tunnel_core::models::connection_log::ConnectionLog) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: l.id,
            user_id: l.user_id,
            peer_ip: l.peer_ip,
            key_fingerprint: l.key_fingerprint,
            attempted_password: l.attempted_password,
            attempted_username: l.attempted_username,
            fail_reason: l.fail_reason,
            success_reason: l.success_reason,
            success: l.success,
            tarpit_method: l.tarpit_method,
            tarpit_action: l.tarpit_action,
            tarpit_threshold_id: l.tarpit_threshold_id,
            banned_by_ban_rule_id: l.banned_by_ban_rule_id,
            started_at: l.started_at.format(&Rfc3339).unwrap_or_default(),
            ended_at: l.ended_at.map(|t| t.format(&Rfc3339).unwrap_or_default()),
        }
    }
}

pub async fn list_connection_logs(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<ConnLogResponse>>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let logs = ConnectionLog::list_for_entity(&state.db, entity_id, 50)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(logs.into_iter().map(ConnLogResponse::from).collect()))
}
