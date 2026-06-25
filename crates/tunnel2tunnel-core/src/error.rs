#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("password hash error: {0}")]
    PasswordHash(String),

    #[error("user not found")]
    UserNotFound,
}
