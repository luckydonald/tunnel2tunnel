use crate::{extractors::AuthUser, AppState, WebError};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tunnel2tunnel_core::models::{
    connection_log::ConnectionLog, entity::Entity, entity_port::EntityPort,
    entity_port_discovery_rule::EntityPortDiscoveryRule, ssh_key::SshKey,
};
use uuid::Uuid;

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EntityListQuery {
    pub entity_type: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateEntityBody {
    pub entity_type: String,
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
    pub name: Option<String>,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
    pub host: Option<String>,
    pub server_entity_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct UpdatePortBody {
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub sort_order: i32,
    pub host: Option<String>,
    pub server_entity_id: Option<Uuid>,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct EntityResponse {
    pub id: Uuid,
    pub entity_type: String,
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
}

impl From<Entity> for EntityResponse {
    fn from(e: Entity) -> Self {
        EntityResponse {
            id: e.id,
            entity_type: e.entity_type,
            name: e.name,
            description: e.description,
            ip_whitelist: e.ip_whitelist,
            valid_until: e.valid_until,
            created_at: e.ts.timestamps.created_at,
            updated_at: e.ts.timestamps.updated_at,
            deleted_at: e.ts.soft_delete.deleted_at,
            online: false,
            last_disconnected_at: None,
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
pub struct EntityPortResponse {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub sort_order: i32,
    pub host: String,
    pub server_entity_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl From<EntityPort> for EntityPortResponse {
    fn from(p: EntityPort) -> Self {
        EntityPortResponse {
            id: p.id,
            entity_id: p.entity_id,
            enabled: p.enabled,
            local_port: p.local_port,
            proxy_port: p.proxy_port,
            name: p.name,
            description: p.description,
            sort_order: p.sort_order,
            host: p.host,
            server_entity_id: p.server_entity_id,
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
    pub ports: Vec<EntityPortResponse>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn require_owner(
    db: &sqlx::PgPool,
    entity_id: Uuid,
    user_id: Uuid,
) -> Result<Entity, WebError> {
    Entity::find_by_id_and_user(db, entity_id, user_id)
        .await?
        .ok_or(WebError::NotFound)
}

// ── Handlers — entities ───────────────────────────────────────────────────────

pub async fn list_entities(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<EntityListQuery>,
) -> Result<Json<Vec<EntityResponse>>, WebError> {
    let list = Entity::list_for_user(&state.db, user.id, q.entity_type.as_deref()).await?;
    let ids: Vec<Uuid> = list.iter().map(|e| e.id).collect();
    let statuses = ConnectionLog::entity_statuses(&state.db, &ids)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(
        list.into_iter()
            .map(|e| {
                let status = statuses.get(&e.id).copied();
                EntityResponse::from(e).with_status(status)
            })
            .collect(),
    ))
}

pub async fn create_entity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(b): Json<CreateEntityBody>,
) -> Result<(StatusCode, Json<EntityResponse>), WebError> {
    if b.entity_type != "server" && b.entity_type != "client" {
        return Err(WebError::BadRequest(
            "entity_type must be 'server' or 'client'".into(),
        ));
    }
    let e = Entity::create(
        &state.db,
        user.id,
        &b.entity_type,
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
    let ports = EntityPort::list_for_entity(&state.db, id).await?;
    let status = ConnectionLog::entity_status(&state.db, id)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(EntityDetailResponse {
        entity: EntityResponse::from(entity).with_status(Some(status)),
        ssh_keys: ssh_keys.into_iter().map(SshKeyResponse::from).collect(),
        ports: ports.into_iter().map(EntityPortResponse::from).collect(),
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
    Ok(Json(EntityResponse::from(e)))
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

// ── Handlers — ports ──────────────────────────────────────────────────────────

pub async fn list_ports(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<EntityPortResponse>>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let ports = EntityPort::list_for_entity(&state.db, entity_id).await?;
    Ok(Json(
        ports.into_iter().map(EntityPortResponse::from).collect(),
    ))
}

pub async fn create_port(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
    Json(b): Json<CreatePortBody>,
) -> Result<(StatusCode, Json<EntityPortResponse>), WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let port = EntityPort::create(
        &state.db,
        entity_id,
        b.enabled.unwrap_or(true),
        b.local_port,
        b.proxy_port,
        b.name.as_deref(),
        b.description.as_deref(),
        b.sort_order.unwrap_or(0),
        b.host.as_deref().unwrap_or("localhost"),
        b.server_entity_id,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(EntityPortResponse::from(port))))
}

pub async fn update_port(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, port_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<UpdatePortBody>,
) -> Result<Json<EntityPortResponse>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    let port = EntityPort::update(
        &state.db,
        port_id,
        entity_id,
        b.enabled,
        b.local_port,
        b.proxy_port,
        b.name.as_deref(),
        b.description.as_deref(),
        b.sort_order,
        b.host.as_deref().unwrap_or("localhost"),
        b.server_entity_id,
    )
    .await?
    .ok_or(WebError::NotFound)?;
    Ok(Json(EntityPortResponse::from(port)))
}

pub async fn delete_port(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((entity_id, port_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    if EntityPort::delete(&state.db, port_id, entity_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Port discovery ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DiscoveredPortResponse {
    #[serde(flatten)]
    pub port: EntityPortResponse,
    pub discovery_state: Option<String>,
    pub client_port_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct ReachableServerResponse {
    #[serde(flatten)]
    pub entity: EntityResponse,
    pub hostname: Option<String>,
    pub ports: Vec<DiscoveredPortResponse>,
}

#[derive(Deserialize)]
pub struct SetDiscoveryStateBody {
    pub state: String,
    pub local_port: Option<i32>,
}

pub async fn list_reachable_servers(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<ReachableServerResponse>>, WebError> {
    let client = require_owner(&state.db, entity_id, user.id).await?;
    if client.entity_type != "client" {
        return Err(WebError::BadRequest("entity must be a client".into()));
    }
    let servers =
        EntityPortDiscoveryRule::list_reachable_for_client(&state.db, entity_id, user.id).await?;
    let response = servers
        .into_iter()
        .map(|s| ReachableServerResponse {
            entity: EntityResponse::from(s.entity),
            hostname: s.hostname,
            ports: s
                .ports
                .into_iter()
                .map(|dp| DiscoveredPortResponse {
                    port: EntityPortResponse::from(dp.port),
                    discovery_state: dp.discovery_state,
                    client_port_id: dp.client_port_id,
                })
                .collect(),
        })
        .collect();
    Ok(Json(response))
}

pub async fn set_port_discovery_state(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((client_id, server_port_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<SetDiscoveryStateBody>,
) -> Result<StatusCode, WebError> {
    let client = require_owner(&state.db, client_id, user.id).await?;
    if client.entity_type != "client" {
        return Err(WebError::BadRequest("entity must be a client".into()));
    }
    match b.state.as_str() {
        "auto" => {
            if let Some(rule) =
                EntityPortDiscoveryRule::delete(&state.db, client_id, server_port_id).await?
            {
                if rule.state == "enabled" {
                    if let Some(cid) = rule.client_port_id {
                        EntityPort::delete(&state.db, cid, client_id).await?;
                    }
                }
            }
        }
        "enabled" => {
            let local_port = b
                .local_port
                .ok_or_else(|| WebError::BadRequest("local_port required for 'enabled'".into()))?;
            let server_port = EntityPort::find_by_id(&state.db, server_port_id)
                .await?
                .ok_or(WebError::NotFound)?;
            // Remove previous client port if re-enabling
            if let Some(existing) =
                EntityPortDiscoveryRule::find(&state.db, client_id, server_port_id).await?
            {
                if existing.state == "enabled" {
                    if let Some(cid) = existing.client_port_id {
                        EntityPort::delete(&state.db, cid, client_id).await?;
                    }
                }
            }
            let client_port = EntityPort::create(
                &state.db,
                client_id,
                true,
                local_port,
                server_port.proxy_port,
                server_port.name.as_deref(),
                server_port.description.as_deref(),
                server_port.sort_order,
                "localhost",
                Some(server_port.entity_id),
            )
            .await?;
            EntityPortDiscoveryRule::upsert(
                &state.db,
                client_id,
                server_port_id,
                "enabled",
                Some(client_port.id),
            )
            .await?;
        }
        "disabled" => {
            if let Some(existing) =
                EntityPortDiscoveryRule::find(&state.db, client_id, server_port_id).await?
            {
                if existing.state == "enabled" {
                    if let Some(cid) = existing.client_port_id {
                        EntityPort::delete(&state.db, cid, client_id).await?;
                    }
                }
            }
            EntityPortDiscoveryRule::upsert(&state.db, client_id, server_port_id, "disabled", None)
                .await?;
        }
        _ => {
            return Err(WebError::BadRequest(
                "state must be 'auto', 'enabled', or 'disabled'".into(),
            ))
        }
    }
    Ok(StatusCode::NO_CONTENT)
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
