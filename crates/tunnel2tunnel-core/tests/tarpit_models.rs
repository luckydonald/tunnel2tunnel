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
        connection_log::{ConnectionLog, ConnectionLogSearch},
        entity::Entity,
        user::User,
    },
};

/// `close_all_open_on_boot` mutates every open row in the shared dev database,
/// which would otherwise race with any concurrently-running test that expects
/// its own row to stay open (`ended_at IS NULL`) — `cargo test` runs tests in
/// this file concurrently by default. Tests that depend on that invariant
/// hold this guard for their duration; the boot-cleanup test holds it too so
/// the two kinds of test can't interleave.
static TEST_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let err =
        sqlx::query("INSERT INTO connection_logs (id, peer_ip, started_at) VALUES ($1, $2, NOW())")
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
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert failing row");
    assert!(!fail_row.success);
    assert_eq!(fail_row.fail_reason.as_deref(), Some("unknown key"));
    assert_eq!(fail_row.success_reason, None);

    let success_row = ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
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
    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        now,
    )
    .await
    .unwrap();
    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        now,
    )
    .await
    .unwrap();
    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        now - TimeDuration::hours(2),
    )
    .await
    .unwrap();

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

    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        now,
    )
    .await
    .unwrap();
    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        now,
    )
    .await
    .unwrap();

    let (rows, total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        Some(false),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .expect("search");
    assert_eq!(total, 1);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].success);
}

#[tokio::test]
async fn search_paginates_and_filters_by_tarpit_method() {
    let pool = test_pool().await;
    let peer_ip = format!("203.0.113.{}", rand_octet());
    let now = OffsetDateTime::now_utc();

    for _ in 0..5 {
        ConnectionLog::create(
            &pool,
            None,
            None,
            Some(&peer_ip),
            None,
            None,
            None,
            Some("unknown key"),
            None,
            Some("slow_auth"),
            None,
            None,
            None,
            now,
        )
        .await
        .unwrap();
    }
    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        now,
    )
    .await
    .unwrap();

    // page 1 of 2 with page_size=2 over the 5 slow_auth rows
    let (page1, total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        Some("slow_auth"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        2,
    )
    .await
    .expect("search page 1");
    assert_eq!(
        total, 5,
        "method filter should exclude the 6th (no tarpit_method) row"
    );
    assert_eq!(page1.len(), 2);

    let (page3, total3) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        Some("slow_auth"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        3,
        2,
    )
    .await
    .expect("search page 3");
    assert_eq!(total3, 5);
    assert_eq!(
        page3.len(),
        1,
        "5 rows at page_size 2 leaves exactly 1 row on page 3"
    );

    let ids_p1: Vec<_> = page1.iter().map(|r| r.id).collect();
    let ids_p3: Vec<_> = page3.iter().map(|r| r.id).collect();
    assert!(
        ids_p1.iter().all(|id| !ids_p3.contains(id)),
        "pages must not overlap"
    );
}

#[tokio::test]
async fn search_filters_tarpit_presence_actions_and_timestamps() {
    let pool = test_pool().await;
    let peer_ip = format!("203.0.113.{}", rand_octet());
    let now = OffsetDateTime::now_utc();
    let old_started = now - TimeDuration::hours(2);
    let trapped_started = now - TimeDuration::hours(1);

    let ordinary = ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        None,
        None,
        None,
        None,
        old_started,
    )
    .await
    .unwrap();
    let trapped = ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("unknown key"),
        None,
        Some("slow_auth"),
        Some("trap"),
        None,
        None,
        trapped_started,
    )
    .await
    .unwrap();
    ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("banned"),
        None,
        None,
        Some("ban"),
        None,
        None,
        now,
    )
    .await
    .unwrap();

    sqlx::query("UPDATE connection_logs SET ended_at = $2 WHERE id = $1")
        .bind(ordinary.id)
        .bind(old_started)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE connection_logs SET ended_at = $2 WHERE id = $1")
        .bind(trapped.id)
        .bind(trapped_started)
        .execute(&pool)
        .await
        .unwrap();

    let (_, method_some_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(method_some_total, 1);

    let (_, method_none_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        Some(false),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(method_none_total, 2);

    let (_, action_some_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(action_some_total, 2);

    let (ban_rows, ban_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        Some("ban"),
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(ban_total, 1);
    assert_eq!(ban_rows[0].tarpit_action.as_deref(), Some("ban"));

    let (_, started_after_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(trapped_started),
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(started_after_total, 2, "the lower bound is inclusive");

    let (_, started_before_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(trapped_started),
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(started_before_total, 2, "the upper bound is inclusive");

    let (_, ended_after_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(trapped_started),
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(
        ended_after_total, 1,
        "open logs do not match ended-time filters"
    );

    let (_, ended_before_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(old_started),
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(
        ended_before_total, 1,
        "the ended-time upper bound is inclusive"
    );

    let deleted = ConnectionLog::delete_matching(
        &pool,
        &ConnectionLogSearch {
            peer_ip: Some(peer_ip.clone()),
            tarpit_action: Some("ban".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(deleted, 1);

    let (_, remaining_total) = ConnectionLog::search(
        &pool,
        Some(&peer_ip),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        50,
    )
    .await
    .unwrap();
    assert_eq!(remaining_total, 2, "only the matching ban log was deleted");
}

#[tokio::test]
async fn entity_statuses_batches_multiple_entities() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let online_entity = Entity::create(
        &pool,
        owner.id,
        Some("batch-online"),
        None,
        None,
        None,
    )
    .await
    .expect("create online entity");
    let offline_entity = Entity::create(
        &pool,
        owner.id,
        Some("batch-offline"),
        None,
        None,
        None,
    )
    .await
    .expect("create offline entity");
    let never_connected = Entity::create(
        &pool,
        owner.id,
        Some("batch-never"),
        None,
        None,
        None,
    )
    .await
    .expect("create never-connected entity");

    ConnectionLog::create(
        &pool,
        Some(online_entity.id),
        Some(owner.id),
        Some("198.51.100.2"),
        None,
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert open session");

    let closed = ConnectionLog::create(
        &pool,
        Some(offline_entity.id),
        Some(owner.id),
        Some("198.51.100.3"),
        None,
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert closed session");
    ConnectionLog::set_ended(&pool, closed.id)
        .await
        .expect("set_ended");

    let statuses = ConnectionLog::entity_statuses(
        &pool,
        &[online_entity.id, offline_entity.id, never_connected.id],
    )
    .await
    .expect("entity_statuses");

    assert!(
        statuses.get(&online_entity.id).unwrap().0,
        "online entity should read online=true"
    );
    assert!(
        !statuses.get(&offline_entity.id).unwrap().0,
        "offline entity should read online=false"
    );
    assert!(
        statuses.get(&offline_entity.id).unwrap().1.is_some(),
        "offline entity should have a last_disconnected_at"
    );
    assert!(
        statuses.get(&never_connected.id).is_none(),
        "an entity with no rows should be absent from the map"
    );
}

#[tokio::test]
async fn entity_status_reflects_open_and_closed_sessions() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let entity = Entity::create(
        &pool,
        owner.id,
        Some("tarpit-test-entity"),
        None,
        None,
        None,
    )
    .await
    .expect("create entity");

    // No sessions yet — offline.
    let (online, last_disconnected_at) = ConnectionLog::entity_status(&pool, entity.id)
        .await
        .unwrap();
    assert!(!online);
    assert_eq!(last_disconnected_at, None);

    let log = ConnectionLog::create(
        &pool,
        Some(entity.id),
        Some(owner.id),
        Some("198.51.100.1"),
        Some("SHA256:abc"),
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert success row");

    let (online, _) = ConnectionLog::entity_status(&pool, entity.id)
        .await
        .unwrap();
    assert!(online, "open successful session should count as online");

    ConnectionLog::set_ended(&pool, log.id)
        .await
        .expect("set_ended");

    let (online, last_disconnected_at) = ConnectionLog::entity_status(&pool, entity.id)
        .await
        .unwrap();
    assert!(!online);
    assert!(last_disconnected_at.is_some());
}

#[tokio::test]
async fn close_all_open_on_boot_closes_only_open_rows() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let owner = test_user(&pool).await;
    let entity = Entity::create(
        &pool,
        owner.id,
        Some("boot-cleanup-entity"),
        None,
        None,
        None,
    )
    .await
    .expect("create entity");

    let open = ConnectionLog::create(
        &pool,
        Some(entity.id),
        Some(owner.id),
        Some("198.51.100.4"),
        None,
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert open session");

    let already_closed = ConnectionLog::create(
        &pool,
        Some(entity.id),
        Some(owner.id),
        Some("198.51.100.5"),
        None,
        None,
        None,
        None,
        Some("correct login"),
        None,
        None,
        None,
        None,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("insert already-closed session");
    ConnectionLog::set_ended(&pool, already_closed.id)
        .await
        .expect("set_ended");
    let already_closed_at = ConnectionLog::list_for_entity(&pool, entity.id, 10)
        .await
        .expect("list_for_entity")
        .into_iter()
        .find(|log| log.id == already_closed.id)
        .expect("already-closed row present")
        .ended_at
        .expect("already-closed row has ended_at");

    let closed = ConnectionLog::close_all_open_on_boot(&pool)
        .await
        .expect("close_all_open_on_boot");
    assert!(closed >= 1, "should report at least the row we just opened");

    let rows = ConnectionLog::list_for_entity(&pool, entity.id, 10)
        .await
        .expect("list_for_entity");

    let open_row = rows
        .iter()
        .find(|log| log.id == open.id)
        .expect("previously-open row present");
    assert!(
        open_row.ended_at.is_some(),
        "previously-open row must now have ended_at set"
    );

    let closed_row = rows
        .iter()
        .find(|log| log.id == already_closed.id)
        .expect("already-closed row present");
    assert_eq!(
        closed_row.ended_at,
        Some(already_closed_at),
        "already-closed row's ended_at must be left untouched"
    );
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
    assert!(
        format!("{err}").contains("ban_rules_scope_type_check")
            || format!("{err}").contains("check")
    );

    // Valid peer_ip rule via the model API.
    let rule = BanRule::create(
        &pool,
        "peer_ip",
        Some("203.0.113.9"),
        None,
        Some("test ban"),
        None,
        admin.id,
        "ban",
    )
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
    BanRule::create(
        &pool,
        "peer_ip",
        Some(&peer_ip),
        None,
        None,
        Some(now - TimeDuration::hours(1)),
        admin.id,
        "ban",
    )
    .await
    .expect("create expired ban rule");
    BanRule::create(
        &pool,
        "peer_ip",
        Some(&peer_ip),
        None,
        None,
        None,
        admin.id,
        "ban",
    )
    .await
    .expect("create indefinite ban rule");

    let active = BanRule::list_active(&pool).await.expect("list_active");
    let matching: Vec<_> = active
        .iter()
        .filter(|r| r.peer_ip.as_deref() == Some(peer_ip.as_str()))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "only the indefinite rule should be active"
    );
    assert!(matching[0].active_until.is_none());
}

fn rand_octet() -> u8 {
    Uuid::now_v7().as_bytes()[15]
}
