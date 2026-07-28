use crate::error::CoreError;
use sqlx::postgres::PgPoolOptions;

pub async fn connect(database_url: &str) -> Result<sqlx::PgPool, CoreError> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(CoreError::Sqlx)
}
