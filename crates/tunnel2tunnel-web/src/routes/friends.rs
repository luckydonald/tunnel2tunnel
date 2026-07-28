use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tunnel2tunnel_core::models::{
    friendship::{Friendship, FriendshipEntityGrant},
    user::User,
};

use crate::{error::WebError, extractors::AuthUser, AppState};

#[derive(Serialize)]
pub struct FriendshipResponse {
    pub id: Uuid,
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub status: String,
    pub visibility_grant: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Friendship> for FriendshipResponse {
    fn from(f: Friendship) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: f.id,
            from_user_id: f.from_user_id,
            to_user_id: f.to_user_id,
            status: f.status,
            visibility_grant: f.visibility_grant,
            created_at: f.ts.created_at.format(&Rfc3339).unwrap_or_default(),
            updated_at: f.ts.updated_at.format(&Rfc3339).unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
pub struct GrantResponse {
    pub id: Uuid,
    pub friendship_id: Uuid,
    pub entity_id: Uuid,
    pub created_at: String,
}

impl From<FriendshipEntityGrant> for GrantResponse {
    fn from(g: FriendshipEntityGrant) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: g.id,
            friendship_id: g.friendship_id,
            entity_id: g.entity_id,
            created_at: g.ts.created_at.format(&Rfc3339).unwrap_or_default(),
        }
    }
}

pub async fn list_friends(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<FriendshipResponse>>, WebError> {
    let rows = Friendship::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(
        rows.into_iter().map(FriendshipResponse::from).collect(),
    ))
}

#[derive(Deserialize)]
pub struct SendRequestBody {
    pub username: String,
}

pub async fn send_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<SendRequestBody>,
) -> Result<(StatusCode, Json<FriendshipResponse>), WebError> {
    let target = User::find_by_username(&state.db, &body.username)
        .await
        .map_err(WebError::Core)?
        .ok_or_else(|| WebError::BadRequest("user not found".into()))?;

    if target.id == user.id {
        return Err(WebError::BadRequest("cannot friend yourself".into()));
    }

    if Friendship::find_between(&state.db, user.id, target.id)
        .await
        .map_err(WebError::Core)?
        .is_some()
    {
        return Err(WebError::BadRequest("friendship already exists".into()));
    }

    let friendship = Friendship::create(&state.db, user.id, target.id)
        .await
        .map_err(WebError::Core)?;

    Ok((
        StatusCode::CREATED,
        Json(FriendshipResponse::from(friendship)),
    ))
}

#[derive(Deserialize)]
pub struct UpdateFriendshipBody {
    pub status: Option<String>,
    pub visibility_grant: Option<String>,
}

pub async fn update_friendship(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFriendshipBody>,
) -> Result<Json<FriendshipResponse>, WebError> {
    let friendship = Friendship::find_by_id(&state.db, id)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)?;

    if friendship.from_user_id != user.id && friendship.to_user_id != user.id {
        return Err(WebError::Forbidden);
    }

    let status = body.status.as_deref().unwrap_or(&friendship.status);
    let visibility = body
        .visibility_grant
        .as_deref()
        .unwrap_or(&friendship.visibility_grant);

    let valid_statuses = ["pending", "accepted", "declined"];
    let valid_visibility = ["none", "clients", "servers", "all"];
    if !valid_statuses.contains(&status) {
        return Err(WebError::BadRequest("invalid status".into()));
    }
    if !valid_visibility.contains(&visibility) {
        return Err(WebError::BadRequest("invalid visibility_grant".into()));
    }

    let updated = Friendship::update_status_and_visibility(&state.db, id, status, visibility)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)?;

    Ok(Json(FriendshipResponse::from(updated)))
}

pub async fn list_grants(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<GrantResponse>>, WebError> {
    let friendship = Friendship::find_by_id(&state.db, id)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)?;

    if friendship.from_user_id != user.id && friendship.to_user_id != user.id {
        return Err(WebError::Forbidden);
    }

    let grants = Friendship::grants_for(&state.db, id)
        .await
        .map_err(WebError::Core)?;

    Ok(Json(grants.into_iter().map(GrantResponse::from).collect()))
}

#[derive(Deserialize)]
pub struct AddGrantBody {
    pub entity_id: Uuid,
}

pub async fn add_grant(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddGrantBody>,
) -> Result<(StatusCode, Json<GrantResponse>), WebError> {
    let friendship = Friendship::find_by_id(&state.db, id)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)?;

    // only from_user can add grants (they're granting visibility of their own entities)
    if friendship.from_user_id != user.id {
        return Err(WebError::Forbidden);
    }

    let grant = Friendship::add_grant(&state.db, id, body.entity_id)
        .await
        .map_err(WebError::Core)?;

    Ok((StatusCode::CREATED, Json(GrantResponse::from(grant))))
}

pub async fn remove_grant(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((id, entity_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WebError> {
    let friendship = Friendship::find_by_id(&state.db, id)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)?;

    if friendship.from_user_id != user.id {
        return Err(WebError::Forbidden);
    }

    let removed = Friendship::remove_grant(&state.db, id, entity_id)
        .await
        .map_err(WebError::Core)?;

    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}
