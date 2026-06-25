use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use uuid::Uuid;
use tunnel2tunnel_core::{auth::verify_password, models::user::User};
use crate::{extractors::AuthUser, AppState, WebError};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub is_admin: bool,
    pub description: Option<String>,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        UserResponse {
            id: u.id,
            username: u.username,
            email: u.email,
            is_admin: u.is_admin,
            description: u.description,
        }
    }
}

pub async fn login(
    session: Session,
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<UserResponse>, WebError> {
    let user = User::find_by_username(&state.db, &body.username)
        .await?
        .ok_or(WebError::Unauthorized)?;

    if user.is_locked {
        return Err(WebError::Forbidden);
    }

    if !verify_password(&body.password, &user.password_hash)? {
        return Err(WebError::Unauthorized);
    }

    session
        .insert("user_id", user.id)
        .await
        .map_err(|e| WebError::Internal(e.to_string()))?;

    Ok(Json(UserResponse::from(user)))
}

pub async fn logout(session: Session) -> Result<Json<serde_json::Value>, WebError> {
    session
        .flush()
        .await
        .map_err(|e| WebError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn me(AuthUser(user): AuthUser) -> Json<UserResponse> {
    Json(UserResponse::from(user))
}
