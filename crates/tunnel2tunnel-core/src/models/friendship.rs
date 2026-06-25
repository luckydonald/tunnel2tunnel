use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Friendship {
    pub id: Uuid,
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub status: String,
    pub visibility_grant: String,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct FriendshipEntityGrant {
    pub id: Uuid,
    pub friendship_id: Uuid,
    pub entity_id: Uuid,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

impl Friendship {
    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, Friendship>(
            "SELECT * FROM friendships \
             WHERE from_user_id = $1 OR to_user_id = $1 \
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Friendship>("SELECT * FROM friendships WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(CoreError::Sqlx)
    }

    pub async fn find_between(
        pool: &PgPool,
        user_a: Uuid,
        user_b: Uuid,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Friendship>(
            "SELECT * FROM friendships \
             WHERE (from_user_id = $1 AND to_user_id = $2) \
                OR (from_user_id = $2 AND to_user_id = $1)",
        )
        .bind(user_a)
        .bind(user_b)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        from_user_id: Uuid,
        to_user_id: Uuid,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, Friendship>(
            "INSERT INTO friendships (id, from_user_id, to_user_id) \
             VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(id)
        .bind(from_user_id)
        .bind(to_user_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn update_status_and_visibility(
        pool: &PgPool,
        id: Uuid,
        status: &str,
        visibility_grant: &str,
    ) -> Result<Option<Self>, CoreError> {
        sqlx::query_as::<_, Friendship>(
            "UPDATE friendships SET status = $2, visibility_grant = $3 \
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(status)
        .bind(visibility_grant)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn grants_for(
        pool: &PgPool,
        friendship_id: Uuid,
    ) -> Result<Vec<FriendshipEntityGrant>, CoreError> {
        sqlx::query_as::<_, FriendshipEntityGrant>(
            "SELECT * FROM friendship_entity_grants \
             WHERE friendship_id = $1 ORDER BY created_at DESC",
        )
        .bind(friendship_id)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn add_grant(
        pool: &PgPool,
        friendship_id: Uuid,
        entity_id: Uuid,
    ) -> Result<FriendshipEntityGrant, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, FriendshipEntityGrant>(
            "INSERT INTO friendship_entity_grants (id, friendship_id, entity_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (friendship_id, entity_id) DO UPDATE SET updated_at = NOW() \
             RETURNING *",
        )
        .bind(id)
        .bind(friendship_id)
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn remove_grant(
        pool: &PgPool,
        friendship_id: Uuid,
        entity_id: Uuid,
    ) -> Result<bool, CoreError> {
        let r = sqlx::query(
            "DELETE FROM friendship_entity_grants WHERE friendship_id = $1 AND entity_id = $2",
        )
        .bind(friendship_id)
        .bind(entity_id)
        .execute(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
