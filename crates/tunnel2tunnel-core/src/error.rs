#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("password hash error: {0}")]
    PasswordHash(String),

    #[error("invalid key format")]
    InvalidKeyFormat,

    #[error("invalid key data (not valid base64)")]
    InvalidKeyData,

    #[error("user not found")]
    UserNotFound,
}
