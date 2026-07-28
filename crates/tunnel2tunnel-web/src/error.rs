use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tunnel2tunnel_core::CoreError;

#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("not found")]
    NotFound,

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Conflict(String),

    #[error("internal error")]
    Internal(String),

    #[error(transparent)]
    Core(#[from] CoreError),
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            WebError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            WebError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
            WebError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            WebError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            WebError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            WebError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
            WebError::Core(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
            ),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}
