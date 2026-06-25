use serde::Serialize;
use time::OffsetDateTime;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Timestamps {
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct SoftDelete {
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TimestampsSoftDelete {
    #[sqlx(flatten)]
    pub timestamps: Timestamps,
    #[sqlx(flatten)]
    pub soft_delete: SoftDelete,
}
