//! Integration tests for the tarpit/ban-rules DB layer: connection_logs
//! CHECK constraints, threshold-counting queries, admin search, entity
//! online-status derivation, and ban_rules CRUD/CHECK enforcement.
//!
//! Requires a reachable PostgreSQL 18 server (see CLAUDE.md's `podman run`
//! instructions). `DATABASE_URL` overrides the default.

use time::{Duration as TimeDuration, OffsetDateTime};
use uuid::Uuid;

use tunnel2tunnel_core::{
    db,
    models::{
        ban_rule::BanRule,
        connection_log::ConnectionLog,
        entity::Entity,
        user::User,
    },
};

async fn test_pool() -> sqlx::PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://t2t:t2t_secret@localhost:5432/tunnel2tunnel".to_string());
    let pool = db::connect(&database_url)
        .await
        .expect("failed to connect to Postgres — see CLAUDE.md for local setup, or set DATABASE_URL");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");
    pool
}

async fn test_user(pool: &sqlx::PgPool) -> User {
    User::create(
        pool,
        &format!("t2t-tarpit-test-{}", Uuid::now_v7()),
        None,
        "irrelevant-password-not-used-by-this-test",
        false,
        Some("scratch user for tarpit_models integration test"),
    )
    .await
    .expect("create test user")
}

#[tokio::test]
async fn connection_log_reason_columns_are_mutually_exclusive() {
    let pool = test_pool().await;
    let peer_ip = format!("203.0.113.{}", rand_octet());

    // Neither reason set — violates the XOR CHECK constraint.
    let err = sqlx::query(
        "INSERT INTO connection_logs (id, peer_ip, started_at) VALUES ($1, $2, NOW())",
    )
    .bind(Uuid::now_v7())
    .bind(&peer_ip)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(format!("{err}").contains("connection_logs_reason_xor_check"));

    // Both reasons set — also violates the XOR CHECK constraint.
    let err = sqlx::query(
        "INSERT INTO connection_logs (id, peer_ip, fail_reason, success_reason, started_at) \
         VALUES ($1, $2, 'unknown key', 'correct login', NOW())",
    )
    .bind(Uuid::now_v7())
    .bind(&peer_ip)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(format!("{err}").contains("connection_logs_reason_xor_check"));
}

#[tokio::test]
async fn connection_log_success_column_matches_reason_set() {
    let pool = test_pool().await;
    let peer_ip = format!("203.0.113.{}", rand_octet());

    let fail_row = ConnectionLog::create(
        &pool, None, None, Some(&peer_ip), None, None,
        Some("unknown key"), None, None, OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert failing row");
    assert!(!fail_row.success);
    assert_eq!(fail_row.fail_reason.as_deref(), Some("unknown key"));
    assert_eq!(fail_row.success_reason, None);

    let success_row = ConnectionLog::create(
        &pool, None, None, Some(&peer_ip), None, None,
        None, Some("correct login"), None, OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert success row");
    assert!(success_row.success);
    assert_eq!(success_row.success_reason.as_deref(), Some("correct login"));
    assert_eq!(success_row.fail_reason, None);
}

#[tokio::test]
async fn count_recent_failures_respects_window() {
    let pool = test_pool().await;
    let peer_ip = format!("203.0.113.{}", rand_octet());
    let now = OffsetDateTime::now_utc();

    // Two failures inside the window, one well outside it.
    ConnectionLog::create(&pool, None, None, Some(&peer_ip), None, None, Some("unknown key"), None, None, now)
        .await.unwrap();
    ConnectionLog::create(&pool, None, None, Some(&peer_ip), None, None, Some("unknown key"), None, None, now)
        .await.unwrap();
    ConnectionLog::create(&pool, None, None, Some(&peer_ip), None, None, Some("unknown key"), None, None, now - TimeDuration::hours(2))
        .await.unwrap();

    let since = now - TimeDuration::minutes(10);
    let count = ConnectionLog::count_recent_failures_for_peer_ip(&pool, &peer_ip, since)
        .await
        .expect("count failures");
    assert_eq!(count, 2);
}

#[tokio::test]
async fn search_filters_by_success_and_peer_ip() {
    let pool = test_pool().await;
    let peer_ip = format!("203.0.113.{}", rand_octet());
    let now = OffsetDateTime::now_utc();

    ConnectionLog::create(&pool, None, None, Some(&peer_ip), None, None, Some("unknown key"), None, None, now)
        .await.unwrap();
    ConnectionLog::create(&pool, None, None, Some(&peer_ip), None, None, None, Some("correct login"), None, now)
        .await.unwrap();

    let (rows, total) = ConnectionLog::search(&pool, Some(&peer_ip), None, Some(false), None, None, 1, 50)
        .await
        .expect("search");
    assert_eq!(total, 1);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].success);
}

#[tokio::test]
async fn entity_status_reflects_open_and_closed_sessions() {
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let entity = Entity::create(&pool, owner.id, "server", Some("tarpit-test-entity"), None, None, None)
        .await
        .expect("create entity");

    // No sessions yet — offline.
    let (online, last_disconnected_at) = ConnectionLog::entity_status(&pool, entity.id).await.unwrap();
    assert!(!online);
    assert_eq!(last_disconnected_at, None);

    let log = ConnectionLog::create(
        &pool, Some(entity.id), Some(owner.id), Some("198.51.100.1"), Some("SHA256:abc"),
        None, None, Some("correct login"), None, OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert success row");

    let (online, _) = ConnectionLog::entity_status(&pool, entity.id).await.unwrap();
    assert!(online, "open successful session should count as online");

    ConnectionLog::set_ended(&pool, log.id).await.expect("set_ended");

    let (online, last_disconnected_at) = ConnectionLog::entity_status(&pool, entity.id).await.unwrap();
    assert!(!online);
    assert!(last_disconnected_at.is_some());
}

#[tokio::test]
async fn ban_rule_scope_check_enforced_both_directions() {
    let pool = test_pool().await;
    let admin = test_user(&pool).await;

    // peer_ip scope with a user_id set — violates the scope CHECK.
    let err = sqlx::query(
        "INSERT INTO ban_rules (id, scope_type, peer_ip, user_id, created_by) \
         VALUES ($1, 'peer_ip', '203.0.113.9', $2, $2)",
    )
    .bind(Uuid::now_v7())
    .bind(admin.id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(format!("{err}").contains("ban_rules_scope_type_check") || format!("{err}").contains("check"));

    // Valid peer_ip rule via the model API.
    let rule = BanRule::create(&pool, "peer_ip", Some("203.0.113.9"), None, Some("test ban"), None, admin.id)
        .await
        .expect("create valid peer_ip ban rule");
    assert_eq!(rule.scope_type, "peer_ip");
}

#[tokio::test]
async fn ban_rule_list_active_excludes_expired() {
    let pool = test_pool().await;
    let admin = test_user(&pool).await;
    let peer_ip = format!("203.0.113.{}", rand_octet());

    let now = OffsetDateTime::now_utc();
    BanRule::create(&pool, "peer_ip", Some(&peer_ip), None, None, Some(now - TimeDuration::hours(1)), admin.id)
        .await
        .expect("create expired ban rule");
    BanRule::create(&pool, "peer_ip", Some(&peer_ip), None, None, None, admin.id)
        .await
        .expect("create indefinite ban rule");

    let active = BanRule::list_active(&pool).await.expect("list_active");
    let matching: Vec<_> = active.iter().filter(|r| r.peer_ip.as_deref() == Some(peer_ip.as_str())).collect();
    assert_eq!(matching.len(), 1, "only the indefinite rule should be active");
    assert!(matching[0].active_until.is_none());
}

fn rand_octet() -> u8 {
    Uuid::now_v7().as_bytes()[15]
}
