//! End-to-end tests for the SSH tarpit/ban-rules feature.
//!
//! Per project instruction, these tests never shell out to a subprocess
//! (unlike `tunnel_e2e.rs`'s OpenSSH-subprocess harness) — everything here
//! drives the real wire protocol via an in-process `russh::client` handler,
//! giving full control over the exact auth sequence without touching
//! `std::process`/`Command`.
//!
//! Requires a reachable PostgreSQL 18 server (see CLAUDE.md). `DATABASE_URL`
//! overrides the default.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::client::{connect_stream, Config as ClientConfig, Handle as ClientHandle};
use russh::keys::key::PrivateKeyWithHashAlg;
use russh::keys::{Algorithm, PrivateKey};
use russh::ChannelMsg;
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpSocket, TcpStream};
use tokio::time::sleep;
use uuid::Uuid;

use tunnel2tunnel_core::{
    db,
    models::{
        connection_log::ConnectionLog, entity::Entity, ssh_key::SshKey,
        tarpit_threshold::TarpitThreshold, user::User,
    },
    pubkey::parse_authorized_keys_line,
};
use tunnel2tunnel_ssh::{start as start_ssh, SshConfig};

/// Asserts that `log.ended_at` was actually stamped, that it isn't nonsensically
/// before `started_at`, and that at least `min_gap` elapsed between the two —
/// e.g. a trap that's supposed to delay the connection before ending it.
fn assert_ended_after_started(log: &ConnectionLog, min_gap: Duration) {
    let ended_at = log
        .ended_at
        .expect("connection_logs row must have ended_at set once its connection closed");
    assert!(
        ended_at >= log.started_at,
        "ended_at ({ended_at:?}) must not be before started_at ({:?})",
        log.started_at
    );
    let gap: Duration = (ended_at - log.started_at)
        .try_into()
        .expect("ended_at >= started_at was just asserted, gap must be non-negative");
    assert!(
        gap >= min_gap,
        "expected at least {min_gap:?} between started_at and ended_at, got {gap:?}"
    );
}

/// All tests in this file share the same loopback peer_ip (127.0.0.1) and
/// the same long-lived dev database, so failure/success counting from one
/// test would otherwise contaminate another's assertions if `cargo test`
/// ran them concurrently (its default). Serialize the whole file with a
/// single guard instead of relying on `--test-threads=1`.
static TEST_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn free_port() -> u16 {
    StdTcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .unwrap()
        .port()
}

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

/// Spawns a fresh t2t SSH server on its own ephemeral port with its own
/// in-process tarpit state (each `start_ssh` call gets a brand new
/// `TarpitState`), so tests don't interfere with each other even though
/// they all connect from the same loopback peer_ip.
async fn spawn_server(pool: sqlx::PgPool) -> u16 {
    let ssh_port = free_port();
    let scratch = std::env::temp_dir().join(format!("t2t-tarpit-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let host_key_path = scratch.join("ssh_host_key").to_str().unwrap().to_string();

    tokio::spawn(async move {
        start_ssh(
            SshConfig {
                ssh_port,
                fail2ban_log_path: None,
                host_key_path,
                host_key_password: None,
            },
            pool,
        )
        .await
        .expect("t2t SSH server failed");
    });

    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", ssh_port)).await.is_ok() {
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    ssh_port
}

fn generate_keypair() -> (PrivateKey, String, String) {
    let key =
        PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519).expect("generate keypair");
    let pub_line = key.public_key().to_openssh().expect("openssh pub line");
    let (algorithm, key_data, _comment) =
        parse_authorized_keys_line(&pub_line).expect("parse pub line");
    (key, algorithm, key_data)
}

/// Minimal client `Handler` — accepts any host key (this is a test against
/// our own server, not a security-sensitive real client) and does nothing
/// else special.
struct TestClient;

impl russh::client::Handler for TestClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn connect_client(port: u16) -> ClientHandle<TestClient> {
    connect_client_at("127.0.0.1", port).await
}

/// Connects to the t2t server on `port`, sourced from `host` — i.e. the
/// server's `peer_ip` for this connection will be exactly `host`. A plain
/// `TcpStream::connect((host, port))` would NOT do this: the destination
/// address doesn't determine which local/source address the kernel picks
/// for an outgoing loopback connection (it always defaults to 127.0.0.1
/// unless the socket is explicitly bound first), so the socket is built by
/// hand here and bound to `host` before connecting.
async fn connect_client_at(host: &str, port: u16) -> ClientHandle<TestClient> {
    let stream = tcp_connect_from(host, port).await;
    connect_stream(Arc::new(ClientConfig::default()), stream, TestClient)
        .await
        .expect("client connect")
}

/// See `connect_client_at` — same "bind the source address first" trick,
/// for tests that talk raw TCP instead of going through `russh::client`.
async fn tcp_connect_from(host: &str, port: u16) -> TcpStream {
    let local: SocketAddr = format!("{host}:0")
        .parse()
        .expect("valid loopback source addr");
    let socket = TcpSocket::new_v4().expect("create v4 socket");
    socket.bind(local).expect("bind to loopback source addr");
    let dest = SocketAddr::from(([127, 0, 0, 1], port));
    socket.connect(dest).await.expect("connect to t2t server")
}

/// A fresh, never-before-used loopback address (127.0.0.0/8 all routes to
/// loopback on Linux) — lets a test drive the peer_ip-scoped tarpit logic
/// without being polluted by (or polluting) any other test's history in the
/// shared dev DB. In particular, `ConnectionLog::peer_ip_has_known_good_history`
/// permanently disables banner-drip eligibility for a peer_ip the first time
/// it ever logs a successful login.
fn random_loopback_ip() -> String {
    let mut buf = [0u8; 3];
    let _ = getrandom::fill(&mut buf);
    format!("127.{}.{}.{}", buf[0].max(1), buf[1], buf[2].max(1))
}

/// Reproduces the reference legitimate-login sequence from
/// `ai/errors/4.legitimiate.txt`: an `auth_none` probe (which every real SSH
/// client sends and which must be rejected without being tarpitted or
/// counted), followed by a real publickey login.
async fn legit_login_sequence(port: u16, key: &PrivateKey) -> (bool, Duration) {
    let started = Instant::now();
    let mut handle = connect_client(port).await;

    // Step 1: `auth_none` probe — must be rejected, but never logged/counted.
    let _ = handle.authenticate_none("probe").await;

    // Step 2 (implicit publickey-offered probe is handled internally by
    // `authenticate_publickey`) + step 3: real publickey auth.
    let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None);
    let result = handle
        .authenticate_publickey("test", key_with_alg)
        .await
        .expect("authenticate_publickey");

    (result.success(), started.elapsed())
}

#[tokio::test]
async fn legit_login_sequence_is_never_tarpitted() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let owner = User::create(
        &pool,
        &format!("t2t-tarpit-e2e-{}", Uuid::now_v7()),
        None,
        "irrelevant-password-not-used-by-this-test",
        false,
        Some("scratch user for tarpit_e2e integration test"),
    )
    .await
    .expect("create test user");
    let entity = Entity::create(
        &pool,
        owner.id,
        Some("tarpit-e2e-legit-client"),
        None,
        None,
        None,
    )
    .await
    .expect("create entity");
    let (key, algorithm, key_data) = generate_keypair();
    SshKey::create(
        &pool,
        entity.id,
        &algorithm,
        &key_data,
        Some("tarpit-e2e"),
        None,
        None,
    )
    .await
    .expect("register key");

    let port = spawn_server(pool.clone()).await;

    let (success, elapsed) = legit_login_sequence(port, &key).await;
    assert!(success, "legitimate publickey login must succeed");
    assert!(
        elapsed < Duration::from_secs(2),
        "legitimate login must not be delayed by any tarpit method, took {elapsed:?}"
    );

    // Give the async ConnectionLog::create() a moment to land.
    sleep(Duration::from_millis(200)).await;
    let logs = ConnectionLog::list_for_entity(&pool, entity.id, 1)
        .await
        .expect("list_for_entity");
    let log = logs
        .first()
        .expect("a connection_logs row must exist for this login");
    assert!(log.success, "login row must be marked successful");
    assert_eq!(log.success_reason.as_deref(), Some("correct login"));
    assert_eq!(
        log.tarpit_method, None,
        "a successful login must never carry a tarpit_method"
    );
    assert_ended_after_started(log, Duration::ZERO);
}

/// A single unregistered-key login attempt, isolated in its own connection.
/// The client `Handle` is dropped when the surrounding scope ends, closing
/// the connection so `T2tHandler`'s `Drop` gets a chance to run.
#[tokio::test]
async fn single_failed_login_marks_ended_at_on_disconnect() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let port = spawn_server(pool.clone()).await;
    let since = OffsetDateTime::now_utc();

    {
        let (bad_key, _, _) = generate_keypair();
        let mut handle = connect_client(port).await;
        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(bad_key), None);
        let result = handle
            .authenticate_publickey("test", key_with_alg)
            .await
            .expect("authenticate_publickey");
        assert!(!result.success(), "unregistered key must never be accepted");
    } // `handle` dropped here -> connection closes

    sleep(Duration::from_millis(200)).await;
    let log: ConnectionLog = sqlx::query_as(
        "SELECT * FROM connection_logs WHERE peer_ip = $1 AND started_at >= $2 \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind("127.0.0.1")
    .bind(since)
    .fetch_one(&pool)
    .await
    .expect("a connection_logs row must exist for this failed attempt");
    assert!(!log.success);
    assert_ended_after_started(&log, Duration::ZERO);
}

#[tokio::test]
async fn auth_none_probe_never_counts_toward_ban() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let port = spawn_server(pool.clone()).await;
    let since = OffsetDateTime::now_utc();

    for _ in 0..10 {
        let mut handle = connect_client(port).await;
        let _ = handle.authenticate_none("probe").await;
    }

    sleep(Duration::from_millis(200)).await;
    let count = ConnectionLog::count_recent_failures_for_peer_ip(&pool, "127.0.0.1", since)
        .await
        .expect("count_recent_failures_for_peer_ip");
    assert_eq!(
        count, 0,
        "auth_none probes must never write a connection_logs row"
    );
}

#[tokio::test]
async fn repeated_bad_key_attempts_trigger_slow_auth_delay() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let port = spawn_server(pool.clone()).await;

    // Default threshold is 5 failed attempts within the window (seeded by
    // migrations/008_tarpit.sql). The 6th attempt should observe the ban
    // that the 5th attempt just triggered.
    let mut last_elapsed = Duration::ZERO;
    for i in 0..6 {
        let (_, unknown_algo, unknown_key_data) = generate_keypair();
        let _ = (unknown_algo, unknown_key_data); // key is never registered — always rejected
        let (bad_key, _, _) = generate_keypair();

        let started = Instant::now();
        let mut handle = connect_client(port).await;
        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(bad_key), None);
        let result = handle
            .authenticate_publickey("test", key_with_alg)
            .await
            .expect("authenticate_publickey");
        assert!(!result.success(), "unregistered key must never be accepted");
        last_elapsed = started.elapsed();

        if i < 5 {
            assert!(
                last_elapsed < Duration::from_secs(2),
                "attempt {i} should not yet be tarpitted, took {last_elapsed:?}"
            );
        }
    }

    assert!(
        last_elapsed >= Duration::from_secs(2),
        "6th attempt should have been slow-auth-delayed after crossing the ban threshold, took {last_elapsed:?}"
    );

    // The 6th attempt's `handle` (scoped to the loop body above) already
    // dropped, closing its connection, so its row's `ended_at` should be set
    // by the time we query it here.
    sleep(Duration::from_millis(200)).await;
    let log: ConnectionLog = sqlx::query_as(
        "SELECT * FROM connection_logs WHERE peer_ip = $1 ORDER BY started_at DESC LIMIT 1",
    )
    .bind("127.0.0.1")
    .fetch_one(&pool)
    .await
    .expect("a connection_logs row must exist for the slow-auth-delayed attempt");
    assert_eq!(log.tarpit_method.as_deref(), Some("slow_auth"));
    // `tarpit::slow_auth::BASE_DELAY` (crates/tunnel2tunnel-ssh/src/tarpit/slow_auth.rs:8)
    // is 3s plus up to 4s of jitter; not reachable from this crate (`mod tarpit;`
    // in tunnel2tunnel-ssh/src/lib.rs is private), so the guaranteed minimum is
    // duplicated here — keep in sync with that const.
    assert_ended_after_started(&log, Duration::from_secs(3));
}

/// Drives `count` failed publickey attempts (always an unregistered key)
/// through the server at `port`, one connection each. Ignores the outcome —
/// works whether the server rejects (slow_auth) or waves the attempt
/// through into a fake shell (`Auth::Accept`), since either way a fresh
/// `connection_logs` failure row is written and the in-memory ban counter
/// advances the same way.
async fn drive_failures(port: u16, count: usize) {
    drive_failures_at("127.0.0.1", port, count).await;
}

async fn drive_failures_at(host: &str, port: u16, count: usize) {
    for _ in 0..count {
        let (bad_key, _, _) = generate_keypair();
        let mut handle = connect_client_at(host, port).await;
        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(bad_key), None);
        let _ = handle.authenticate_publickey("test", key_with_alg).await;
    }
}

/// `TarpitMethod::round_robin` cycles BannerDrip(0) -> SlowAuth(1) ->
/// FakeShell(2) -> BannerDrip(3) by `trigger_count`, and each ban-threshold
/// crossing (default: 5 failures) bumps `trigger_count` by one. So the 3rd
/// crossing (15 failures total) lands back on BannerDrip for the *next*
/// connection — this drives real traffic through the whole pipeline to
/// prove the accept-time hook in `start()` actually engages the raw-socket
/// drip path, not just the pure round-robin math already unit-tested.
#[tokio::test]
async fn repeated_bans_eventually_engage_banner_drip() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let port = spawn_server(pool.clone()).await;

    // A fresh loopback address, never seen before in the shared dev DB, so
    // this test's peer_ip can't have already been marked as having "known
    // good history" (permanently disabling banner-drip) by some earlier
    // test or session that logged a successful login from plain 127.0.0.1.
    let peer_ip = random_loopback_ip();

    drive_failures_at(&peer_ip, port, 15).await;

    let mut socket = tcp_connect_from(&peer_ip, port).await;
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(15), socket.read(&mut buf))
        .await
        .expect("expected at least one banner-drip line within the timeout")
        .expect("read banner-drip line");
    assert!(n > 0, "connection closed with no data");
    let text = String::from_utf8_lossy(&buf[..n]);
    assert!(
        !text.starts_with("SSH-"),
        "banner-drip must never send the real SSH-2.0 identification line, got: {text:?}"
    );

    // Force the server's drip loop to notice the connection is gone: closing
    // our end makes its `write_all` fail, at which point it stamps `ended_at`
    // (crates/tunnel2tunnel-ssh/src/tarpit/banner_drip.rs). A write right
    // after our FIN often still succeeds locally (the RST only comes back
    // after that write, and gets surfaced as an error on the *next* one), so
    // the server may need two drip ticks — not just one — to actually
    // observe the failure and break its loop.
    drop(socket);
    // `DRIP_INTERVAL` (crates/tunnel2tunnel-ssh/src/tarpit/banner_drip.rs:24)
    // is a private 10s const, unreachable from this crate — duplicated here,
    // keep in sync with that const.
    sleep(Duration::from_secs(21)).await;
    let log: ConnectionLog = sqlx::query_as(
        "SELECT * FROM connection_logs WHERE peer_ip = $1 AND tarpit_method = 'banner_drip' \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(&peer_ip)
    .fetch_one(&pool)
    .await
    .expect("a connection_logs row must exist for the banner-drip trap");
    assert_ended_after_started(&log, Duration::from_secs(10));
}

/// After 2 ban-threshold crossings (10 failures), `trigger_count == 2` and
/// `round_robin(2) == FakeShell` — the next attempt should be waved through
/// with `Auth::Accept` into a bogus shell rather than rejected, and opening
/// a channel on it should yield the fake `$ ` prompt bytes rather than any
/// real access.
#[tokio::test]
async fn repeated_bans_eventually_engage_fake_shell() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let port = spawn_server(pool.clone()).await;

    drive_failures(port, 10).await;

    let (bad_key, _, _) = generate_keypair();
    let mut handle = connect_client(port).await;
    let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(bad_key), None);
    let result = handle
        .authenticate_publickey("test", key_with_alg)
        .await
        .expect("authenticate_publickey");
    assert!(
        result.success(),
        "an unregistered key on the fake-shell round-robin slot must still get Auth::Accept"
    );

    let mut channel = handle
        .channel_open_session()
        .await
        .expect("open session channel on the fake shell");
    let msg = tokio::time::timeout(Duration::from_secs(5), channel.wait())
        .await
        .expect("expected the fake shell to send its bogus prompt")
        .expect("channel closed with no data");
    let ChannelMsg::Data { data } = msg else {
        panic!("expected a Data message with the bogus prompt, got: {msg:?}");
    };
    assert!(
        String::from_utf8_lossy(&data).contains('$'),
        "expected the fake shell's bogus '$ ' prompt, got: {data:?}"
    );

    // Closing the connection should let `T2tHandler`'s `Drop` mark this
    // fake-shell trap's row ended. Drop the channel first — the client
    // `Handle` alone doesn't necessarily tear down the TCP connection while
    // a `Channel` derived from it is still alive.
    drop(channel);
    drop(handle);
    sleep(Duration::from_millis(200)).await;
    let log: ConnectionLog = sqlx::query_as(
        "SELECT * FROM connection_logs WHERE tarpit_action = 'trap' AND tarpit_method = 'fake_shell' \
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("a connection_logs row must exist for the fake-shell trap");
    assert_ended_after_started(&log, Duration::ZERO);
}

/// A threshold rule configured with `action = "ban"` must reject instantly —
/// no slow-auth delay, no fake-shell `Auth::Accept`, not even the real
/// SSH-2.0 identification line — once its own (tighter) count/window trips,
/// independent of the default trap-action rule seeded by earlier migrations.
#[tokio::test]
async fn ban_action_threshold_rejects_instantly_without_tarpit() {
    let _guard = TEST_GUARD.lock().await;
    let pool = test_pool().await;
    let threshold = TarpitThreshold::create(&pool, 2, 600, true, "ban")
        .await
        .expect("create ban-action threshold");

    // Inserted before spawn_server, so the server's synchronous first
    // refresh (awaited before it starts accepting) already sees this rule.
    let port = spawn_server(pool.clone()).await;

    drive_failures(port, 2).await;
    sleep(Duration::from_millis(200)).await;

    let started = Instant::now();
    let mut socket = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to hard-banned port");
    let mut buf = vec![0u8; 4096];
    let result = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buf)).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "hard ban must close the connection instantly, not delay it, took {elapsed:?}"
    );
    match result {
        Ok(Ok(0)) => {}  // connection closed with no data — expected
        Ok(Err(_)) => {} // read error from an abrupt close is also acceptable
        Ok(Ok(n)) => panic!(
            "hard ban must never send any data (no tarpit method engages), got {n} bytes: {:?}",
            &buf[..n]
        ),
        Err(_) => panic!("expected the connection to close quickly, but read timed out"),
    }

    // `log_hard_ban` (crates/tunnel2tunnel-ssh/src/tarpit/mod.rs) is
    // `tokio::spawn`ed from the accept loop rather than awaited inline, so
    // give it a moment to land before checking the row.
    sleep(Duration::from_millis(200)).await;
    let log: ConnectionLog = sqlx::query_as(
        "SELECT * FROM connection_logs WHERE tarpit_action = 'ban' ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("a connection_logs row must exist for the hard ban");
    assert_ended_after_started(&log, Duration::ZERO);

    TarpitThreshold::soft_delete(&pool, threshold.id).await.ok();
}
