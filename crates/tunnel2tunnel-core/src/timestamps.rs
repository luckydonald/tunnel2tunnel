use serde::Serialize;
use time::OffsetDateTime;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Timestamps {
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct SoftDelete {
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TimestampsSoftDelete {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub timestamps: Timestamps,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub soft_delete: SoftDelete,
}
