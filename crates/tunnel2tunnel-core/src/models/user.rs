use serde::Serialize;
use uuid::Uuid;
use sqlx::PgPool;
use crate::error::CoreError;
use crate::timestamps::TimestampsSoftDelete;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    #[serde(skip)]
    pub password_hash: String,
    pub is_admin: bool,
    pub is_locked: bool,
    pub description: Option<String>,
    #[sqlx(flatten)]
    pub ts: TimestampsSoftDelete,
}

impl User {
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn find_by_username(pool: &PgPool, username: &str) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        username: &str,
        email: Option<&str>,
        password: &str,
        is_admin: bool,
        description: Option<&str>,
    ) -> Result<Self, CoreError> {
        let password_hash = crate::auth::hash_password(password)?;
        let id = Uuid::now_v7();
        sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (id, username, email, password_hash, is_admin, is_locked, description)
            VALUES ($1, $2, $3, $4, $5, false, $6)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(username)
        .bind(email)
        .bind(&password_hash)
        .bind(is_admin)
        .bind(description)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn exists_by_username(pool: &PgPool, username: &str) -> Result<bool, CoreError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM users WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(count > 0)
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE deleted_at IS NULL ORDER BY created_at",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        email: Option<&str>,
        is_admin: bool,
        is_locked: bool,
        description: Option<&str>,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, User>(
            "UPDATE users \
             SET email = $2, is_admin = $3, is_locked = $4, description = $5 \
             WHERE id = $1 AND deleted_at IS NULL RETURNING *",
        )
        .bind(id)
        .bind(email)
        .bind(is_admin)
        .bind(is_locked)
        .bind(description)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn update_password(
        pool: &PgPool,
        id: Uuid,
        new_password: &str,
    ) -> Result<bool, CoreError> {
        let hash = crate::auth::hash_password(new_password)?;
        let r = sqlx::query(
            "UPDATE users SET password_hash = $2 WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&hash)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn soft_delete(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "UPDATE users SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn count_admins_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<i64, CoreError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE is_admin = true AND deleted_at IS NULL FOR UPDATE",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(count)
    }
}
