use axum::{
    extract::FromRequestParts,
    http::request::Parts,
};
use tower_sessions::Session;
use uuid::Uuid;
use tunnel2tunnel_core::models::user::User;
use crate::{AppState, WebError};

pub struct AuthUser(pub User);
pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = WebError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| WebError::Unauthorized)?;

        let user_id: Option<Uuid> = session
            .get("user_id")
            .await
            .map_err(|_| WebError::Unauthorized)?;

        let user_id = user_id.ok_or(WebError::Unauthorized)?;

        let user = User::find_by_id(&state.db, user_id)
            .await
            .map_err(WebError::Core)?
            .ok_or(WebError::Unauthorized)?;

        if user.is_locked {
            return Err(WebError::Forbidden);
        }

        Ok(AuthUser(user))
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = WebError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin {
            return Err(WebError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}
