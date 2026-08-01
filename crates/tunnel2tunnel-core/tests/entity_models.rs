//! Integration tests for `Entity::find_unique_id_by_name`.
//!
//! Requires a reachable PostgreSQL 18 server (see CLAUDE.md's `podman run`
//! instructions). `DATABASE_URL` overrides the default.

use uuid::Uuid;

use tunnel2tunnel_core::{
    db,
    models::{entity::Entity, user::User},
};

async fn test_pool() -> sqlx::PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://t2t:t2t_secret@localhost:5432/tunnel2tunnel".to_string());
    let pool = db::connect(&database_url).await.expect(
        "failed to connect to Postgres — see CLAUDE.md for local setup, or set DATABASE_URL",
    );
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");
    pool
}

async fn test_user(pool: &sqlx::PgPool) -> User {
    User::create(
        pool,
        &format!("t2t-entity-test-{}", Uuid::now_v7()),
        None,
        "irrelevant-password-not-used-by-this-test",
        false,
        Some("scratch user for entity_models integration test"),
    )
    .await
    .expect("create test user")
}

#[tokio::test]
async fn find_unique_id_by_name_resolves_an_unambiguous_name() {
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let name = format!("solo-{}", Uuid::now_v7());
    let entity = Entity::create(&pool, owner.id, Some(&name), None, None, None)
        .await
        .expect("create entity");

    let resolved = Entity::find_unique_id_by_name(&pool, &name)
        .await
        .expect("query should succeed");
    assert_eq!(resolved, Some(entity.id));
}

#[tokio::test]
async fn find_unique_id_by_name_is_none_for_an_ambiguous_name() {
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let name = format!("dup-{}", Uuid::now_v7());
    Entity::create(&pool, owner.id, Some(&name), None, None, None)
        .await
        .expect("create first entity");
    Entity::create(&pool, owner.id, Some(&name), None, None, None)
        .await
        .expect("create second entity");

    let resolved = Entity::find_unique_id_by_name(&pool, &name)
        .await
        .expect("query should succeed");
    assert_eq!(resolved, None);
}

#[tokio::test]
async fn find_unique_id_by_name_ignores_soft_deleted_entities() {
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let name = format!("deleted-{}", Uuid::now_v7());
    let entity = Entity::create(&pool, owner.id, Some(&name), None, None, None)
        .await
        .expect("create entity");
    Entity::soft_delete(&pool, entity.id, owner.id)
        .await
        .expect("soft delete entity");

    let resolved = Entity::find_unique_id_by_name(&pool, &name)
        .await
        .expect("query should succeed");
    assert_eq!(resolved, None);
}

#[tokio::test]
async fn find_unique_id_by_name_is_none_for_unknown_name() {
    let pool = test_pool().await;
    let resolved = Entity::find_unique_id_by_name(&pool, &format!("nope-{}", Uuid::now_v7()))
        .await
        .expect("query should succeed");
    assert_eq!(resolved, None);
}
