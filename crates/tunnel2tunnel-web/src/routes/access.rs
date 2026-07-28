use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tunnel2tunnel_core::models::{entity::Entity, entity_access::EntityAccess};

use crate::{error::WebError, extractors::AuthUser, AppState};

#[derive(Serialize)]
pub struct AccessResponse {
    pub id: Uuid,
    pub owner_entity_id: Uuid,
    pub subject_type: String,
    pub subject_entity_id: Option<Uuid>,
    pub subject_user_id: Option<Uuid>,
    pub hostname: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<EntityAccess> for AccessResponse {
    fn from(a: EntityAccess) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: a.id,
            owner_entity_id: a.owner_entity_id,
            subject_type: a.subject_type,
            subject_entity_id: a.subject_entity_id,
            subject_user_id: a.subject_user_id,
            hostname: a.hostname,
            created_at: a.ts.created_at.format(&Rfc3339).unwrap_or_default(),
            updated_at: a.ts.updated_at.format(&Rfc3339).unwrap_or_default(),
        }
    }
}

async fn require_entity_owner(
    state: &AppState,
    entity_id: Uuid,
    user_id: Uuid,
) -> Result<Entity, WebError> {
    Entity::find_by_id_and_user(&state.db, entity_id, user_id)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)
}

pub async fn list_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<AccessResponse>>, WebError> {
    require_entity_owner(&state, entity_id, user.id).await?;
    let rows = EntityAccess::list_for_entity(&state.db, entity_id)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(rows.into_iter().map(AccessResponse::from).collect()))
}

pub async fn list_incoming_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<Vec<AccessResponse>>, WebError> {
    require_entity_owner(&state, entity_id, user.id).await?;
    let rows = EntityAccess::list_incoming(&state.db, entity_id)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(rows.into_iter().map(AccessResponse::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateAccessBody {
    pub subject_type: String,
    pub subject_entity_id: Option<Uuid>,
    pub subject_user_id: Option<Uuid>,
    pub hostname: Option<String>,
}

pub async fn create_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(entity_id): Path<Uuid>,
    Json(body): Json<CreateAccessBody>,
) -> Result<(StatusCode, Json<AccessResponse>), WebError> {
    require_entity_owner(&state, entity_id, user.id).await?;

    let valid_types = ["entity", "all_mine", "all_user_entities", "public_lite"];
    if !valid_types.contains(&body.subject_type.as_str()) {
        return Err(WebError::BadRequest("invalid subject_type".into()));
    }

    let rule = EntityAccess::create(
        &state.db,
        entity_id,
        &body.subject_type,
        body.subject_entity_id,
        body.subject_user_id,
        body.hostname.as_deref(),
    )
    .await
    .map_err(WebError::Core)?;

    Ok((StatusCode::CREATED, Json(AccessResponse::from(rule))))
}

pub async fn delete_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((entity_id, rule_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    require_entity_owner(&state, entity_id, user.id).await?;
    let deleted = EntityAccess::delete(&state.db, rule_id, entity_id)
        .await
        .map_err(WebError::Core)?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}
