use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;
use tunnel2tunnel_core::models::{
    entity::Entity,
    entity_port::EntityPort,
    ssh_key::SshKey,
};
use crate::{extractors::AuthUser, AppState, WebError};

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
}

#[derive(Deserialize)]
pub struct UpdatePortBody {
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub sort_order: i32,
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
        }
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
    Ok(Json(list.into_iter().map(EntityResponse::from).collect()))
}

pub async fn create_entity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(b): Json<CreateEntityBody>,
) -> Result<(StatusCode, Json<EntityResponse>), WebError> {
    if b.entity_type != "server" && b.entity_type != "client" {
        return Err(WebError::BadRequest("entity_type must be 'server' or 'client'".into()));
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
    Ok(Json(EntityDetailResponse {
        entity: EntityResponse::from(entity),
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
    Ok(Json(ports.into_iter().map(EntityPortResponse::from).collect()))
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
