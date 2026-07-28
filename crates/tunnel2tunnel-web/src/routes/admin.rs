use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tunnel2tunnel_core::models::user::User;

use crate::{error::WebError, extractors::AdminUser, AppState};

#[derive(Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub is_admin: bool,
    pub is_locked: bool,
    pub description: Option<String>,
    pub created_at: String,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            is_admin: u.is_admin,
            is_locked: u.is_locked,
            description: u.description,
            created_at: u
                .ts
                .timestamps
                .created_at
                .format(&Rfc3339)
                .unwrap_or_default(),
        }
    }
}

pub async fn list_users(
    State(state): State<AppState>,
    AdminUser(_admin): AdminUser,
) -> Result<Json<Vec<UserResponse>>, WebError> {
    let users = User::list_all(&state.db).await.map_err(WebError::Core)?;
    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateUserBody {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
    pub is_admin: Option<bool>,
    pub description: Option<String>,
}

pub async fn create_user(
    State(state): State<AppState>,
    AdminUser(_admin): AdminUser,
    Json(body): Json<CreateUserBody>,
) -> Result<(StatusCode, Json<UserResponse>), WebError> {
    if User::exists_by_username(&state.db, &body.username)
        .await
        .map_err(WebError::Core)?
    {
        return Err(WebError::Conflict("username already taken".into()));
    }

    let user = User::create(
        &state.db,
        &body.username,
        body.email.as_deref(),
        &body.password,
        body.is_admin.unwrap_or(false),
        body.description.as_deref(),
    )
    .await
    .map_err(WebError::Core)?;

    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

#[derive(Deserialize)]
pub struct UpdateUserBody {
    pub email: Option<String>,
    pub is_admin: Option<bool>,
    pub is_locked: Option<bool>,
    pub description: Option<String>,
    pub password: Option<String>,
}

pub async fn update_user(
    State(state): State<AppState>,
    AdminUser(_admin): AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserBody>,
) -> Result<Json<UserResponse>, WebError> {
    let target = User::find_by_id(&state.db, id)
        .await
        .map_err(WebError::Core)?
        .ok_or(WebError::NotFound)?;

    let new_is_admin = body.is_admin.unwrap_or(target.is_admin);
    let new_is_locked = body.is_locked.unwrap_or(target.is_locked);

    // Self-demote guard: if demoting self (or the target admin), ensure ≥2 admins
    if target.is_admin && !new_is_admin {
        let mut tx = state.db.begin().await.map_err(core_error)?;
        let count = User::count_admins_tx(&mut tx)
            .await
            .map_err(WebError::Core)?;
        if count <= 1 {
            let _ = tx.rollback().await;
            return Err(WebError::Conflict("cannot demote the last admin".into()));
        }
        let _ = tx.rollback().await;
    }

    // Update password if requested
    if let Some(ref pw) = body.password {
        User::update_password(&state.db, id, pw)
            .await
            .map_err(WebError::Core)?;
    }

    let updated = User::update(
        &state.db,
        id,
        body.email.as_deref().or(target.email.as_deref()),
        new_is_admin,
        new_is_locked,
        body.description
            .as_deref()
            .or(target.description.as_deref()),
    )
    .await
    .map_err(WebError::Core)?
    .ok_or(WebError::NotFound)?;

    Ok(Json(UserResponse::from(updated)))
}

fn core_error(e: sqlx::Error) -> WebError {
    WebError::Core(tunnel2tunnel_core::CoreError::Sqlx(e))
}
