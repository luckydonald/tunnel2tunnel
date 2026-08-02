//! End-to-end regression test for the SSH rendezvous tunnel, automating the
//! manual procedure documented in `MANUAL_TESTING.md`: one entity registers a
//! remote forward (`ssh -R`), another consumes it (`ssh -L`), and traffic
//! must flow through t2t between the two independent SSH connections.
//!
//! A real `ssh` client binary is used for both legs (not a Rust SSH client)
//! because this is exactly what caught the forwarded-tcpip address bug this
//! test guards against: OpenSSH matches incoming forwarded-tcpip requests
//! against its own registered `(address, port)` table, and only a real
//! OpenSSH client reproduces that matching behavior.
//!
//! The test drives two separate server entities (each with its own
//! generated keypair and its own fixture service) through the same client
//! entity, and sends several distinct requests — not just repeats of the
//! same GET — through each tunnel: this proves both that request/response
//! framing survives varied traffic (different paths, a `POST` with a body)
//! and that routing picks the *correct* backend when more than one server
//! is registered.
//!
//! Requires a reachable PostgreSQL 18 server. `DATABASE_URL` overrides the
//! default, which matches the connection string set up in
//! `MANUAL_TESTING.md` step 1. All scratch files (SSH host key, generated
//! keypairs) live under a fresh directory in `std::env::temp_dir()`, same as
//! the manual procedure's `/tmp/t2t-test-run`.

use std::net::TcpListener as StdTcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::sleep;
use uuid::Uuid;

use tunnel2tunnel_core::{
    db,
    models::{
        entity::Entity, entity_access::EntityAccess, port_config::PortConfig,
        port_subscription::PortSubscription, ssh_key::SshKey, user::User,
    },
    pubkey::parse_authorized_keys_line,
};
use tunnel2tunnel_ssh::{
    new_active_tunnels, new_server_slots, start as start_ssh, ActiveTunnelInfo, ActiveTunnels,
    SshConfig,
};

/// The remote-forward routing keys used between the `ssh` legs. Neither is
/// ever bound as a real OS socket by t2t (see `tcpip_forward` in
/// `tunnel2tunnel-ssh/src/lib.rs`) — they're purely the map keys t2t uses to
/// pair a `-L` request with the matching `-R` registration, so fixed
/// constants are safe even under parallel test runs. Two distinct values are
/// needed since both server entities register a forward at the same time.
const PROXY_PORT_A: u16 = 19123;
const PROXY_PORT_B: u16 = 19124;

fn free_port() -> u16 {
    StdTcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .unwrap()
        .port()
}

/// Kills the wrapped `ssh` subprocess on drop, so a panicking assertion never
/// leaks a live child process.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}

/// Prints child stderr line-by-line (prefixed) for debugging test failures,
/// without blocking the caller.
fn log_child_stderr(name: &'static str, child: &mut Child) {
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[{name}] {line}");
            }
        });
    }
}

/// Generates a fresh Ed25519 keypair, writes the private key to `priv_path`
/// in OpenSSH format (for `ssh -i`), and returns `(algorithm, key_data_b64)`
/// ready for `SshKey::create`. Same approach as `load_or_generate_host_key`
/// in `tunnel2tunnel-ssh/src/lib.rs` uses for the server's own host key.
fn generate_test_keypair(priv_path: &Path) -> anyhow::Result<(String, String)> {
    let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)?;
    key.write_openssh_file(priv_path.to_str().unwrap(), LineEnding::LF)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(priv_path, std::fs::Permissions::from_mode(0o600))?;
    }
    let pub_line = key.public_key().to_openssh()?;
    let (algorithm, key_data, _comment) = parse_authorized_keys_line(&pub_line)?;
    Ok((algorithm, key_data))
}

/// Builds a bare HTTP response with `body` as its content.
fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Local fixture "service" that a server entity forwards to. Every response
/// body is prefixed with `label` (e.g. `"A"` / `"B"`) so that when two
/// fixture services are running side by side (one per server entity), a
/// response can never be mistaken for coming from the wrong one — this is
/// what lets the test assert on routing, not just on the pipe staying open.
/// Routes:
/// - `GET /`      -> `"{label} hello from the tunneled service"`
/// - `GET /alpha` -> `"{label} alpha response body"`
/// - `GET /beta`  -> `"{label} beta response body"`
/// - `POST /echo` -> echoes the request body back verbatim, unprefixed —
///   exercises client->server bytes flowing through the tunnel, not just
///   server->client.
/// - anything else -> `404`.
/// Runs for the test's lifetime.
async fn spawn_fixture_service(label: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture service");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&buf[..n]);
                let mut lines = request.split("\r\n");
                let request_line = lines.next().unwrap_or_default();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default();
                let path = parts.next().unwrap_or_default();

                let response = match (method, path) {
                    ("GET", "/") => http_response(
                        "200 OK",
                        &format!("{label} hello from the tunneled service"),
                    ),
                    ("GET", "/alpha") => {
                        http_response("200 OK", &format!("{label} alpha response body"))
                    }
                    ("GET", "/beta") => {
                        http_response("200 OK", &format!("{label} beta response body"))
                    }
                    ("POST", "/echo") => {
                        // Body follows the blank line that ends the headers.
                        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
                        http_response("200 OK", body)
                    }
                    _ => http_response("404 Not Found", ""),
                };
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    port
}

/// Issues a single HTTP request (`method`/`path`/`body`) through `port` and
/// returns the raw response text.
async fn http_request(port: u16, method: &str, path: &str, body: &str) -> anyhow::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: t2t-test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(request.as_bytes()).await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Repeatedly issues `method path` (with `body`, if any) through `port`,
/// retrying until `expect_contains` shows up in the response or `timeout`
/// elapses. A fresh connection is used per attempt because a rejected
/// direct-tcpip channel closes whatever local connection triggered it,
/// without killing the `ssh -L` process itself — so early attempts made
/// before the `-R` side has registered are expected to fail and should just
/// be retried.
async fn fetch_through_tunnel(
    port: u16,
    method: &str,
    path: &str,
    body: &str,
    expect_contains: &str,
    timeout: Duration,
) -> anyhow::Result<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let attempt = http_request(port, method, path, body).await;

        match attempt {
            Ok(response) if response.contains(expect_contains) => return Ok(response),
            _ if tokio::time::Instant::now() < deadline => sleep(Duration::from_millis(150)).await,
            Ok(response) => {
                anyhow::bail!(
                    "tunnel never returned the expected response; last response: {response:?}"
                )
            }
            Err(e) => anyhow::bail!("tunnel never became reachable: {e}"),
        }
    }
}

/// Spawns `ssh -N -R proxy_port:127.0.0.1:fixture_port` against the t2t
/// server, using `key_path` for auth. `label` is only used to prefix the
/// child's stderr lines for debugging.
fn spawn_ssh_r(
    key_path: &Path,
    ssh_port: u16,
    proxy_port: u16,
    fixture_port: u16,
    label: &'static str,
) -> ChildGuard {
    let mut child = Command::new("ssh")
        .args([
            "-N",
            "-R",
            &format!("{proxy_port}:127.0.0.1:{fixture_port}"),
        ])
        .args(["-i", key_path.to_str().unwrap()])
        .args(["-p", &ssh_port.to_string()])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .arg("server@127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ssh -R");
    log_child_stderr(label, &mut child);
    ChildGuard(child)
}

/// Spawns `ssh -N -L local_port:target_entity_id:proxy_port` against the t2t
/// server, using `key_path` for auth. `label` is only used to prefix the
/// child's stderr lines for debugging.
fn spawn_ssh_l(
    key_path: &Path,
    ssh_port: u16,
    local_port: u16,
    target_entity_id: Uuid,
    proxy_port: u16,
    label: &'static str,
) -> ChildGuard {
    let mut child = Command::new("ssh")
        .args([
            "-N",
            "-L",
            &format!("{local_port}:{target_entity_id}:{proxy_port}"),
        ])
        .args(["-i", key_path.to_str().unwrap()])
        .args(["-p", &ssh_port.to_string()])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .arg("client@127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ssh -L");
    log_child_stderr(label, &mut child);
    ChildGuard(child)
}

/// Polls until `tcpip_forward`'s auto-created `port_configs` row for
/// `(entity_id, proxy_port)` shows up, or panics after 10s.
async fn wait_for_port_config(pool: &sqlx::PgPool, entity_id: Uuid, proxy_port: u16) -> PortConfig {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(pc) =
            PortConfig::find_enabled_by_entity_and_proxy_port(pool, entity_id, proxy_port as i32)
                .await
                .expect("query port_config")
        {
            return pc;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "port_config was never auto-created for the -R forward"
        );
        sleep(Duration::from_millis(100)).await;
    }
}

/// Polls `active_tunnels` until an entry matching the given identifiers
/// appears, or panics after `timeout`. The fixture service always
/// `shutdown()`s its side of the connection right after answering a
/// request, which tears the bridge — and its `active_tunnels` entry — back
/// down almost immediately, so callers must keep a connection through the
/// relevant tunnel alive (see `TcpStream::connect` without sending/reading
/// anything) for the whole window this is polling across.
async fn wait_for_active_tunnel_entry(
    active_tunnels: &ActiveTunnels,
    client_entity_id: Uuid,
    target_entity_id: Uuid,
    port_config_id: Uuid,
    timeout: Duration,
) -> ActiveTunnelInfo {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let found = active_tunnels
            .lock()
            .await
            .values()
            .find(|info| {
                info.client_entity_id == client_entity_id
                    && info.target_entity_id == target_entity_id
                    && info.port_config_id == port_config_id
            })
            .cloned();
        if let Some(info) = found {
            return info;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected an active_tunnels entry for target_entity {target_entity_id}"
        );
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn two_ssh_connections_tunnel_through_rendezvous() {
    if Command::new("ssh").arg("-V").output().await.is_err() {
        eprintln!(
            "skipping two_ssh_connections_tunnel_through_rendezvous: no `ssh` binary in PATH"
        );
        return;
    }

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://t2t:t2t_secret@localhost:5432/tunnel2tunnel".to_string());
    let pool = db::connect(&database_url).await.expect(
        "failed to connect to Postgres — see MANUAL_TESTING.md step 1 to start one locally, \
         or set DATABASE_URL",
    );
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

    // Scratch dir for the host key + generated keypairs, mirroring
    // MANUAL_TESTING.md's `/tmp/t2t-test-run`.
    let scratch = std::env::temp_dir().join(format!("t2t-e2e-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let host_key_path = scratch.join("ssh_host_key");
    let server_a_key_path = scratch.join("server_a_key");
    let server_b_key_path = scratch.join("server_b_key");
    let client_key_path = scratch.join("client_key");

    // Three entities, owned by a fresh dedicated user so repeated runs never
    // collide with each other or with unrelated data in the same database:
    // two independent server profiles (each with its own key) and one
    // client profile that talks to both, so the test can assert on routing
    // (does the client's request for server A ever leak to server B?) and
    // not just on a single pipe staying open.
    let owner = User::create(
        &pool,
        &format!("t2t-e2e-test-{}", Uuid::now_v7()),
        None,
        "irrelevant-password-not-used-by-this-test",
        false,
        Some("scratch user for tunnel_e2e integration test"),
    )
    .await
    .expect("create test user");

    let server_entity_a =
        Entity::create(&pool, owner.id, Some("e2e-test-server-a"), None, None, None)
            .await
            .expect("create server entity A");
    let server_entity_b =
        Entity::create(&pool, owner.id, Some("e2e-test-server-b"), None, None, None)
            .await
            .expect("create server entity B");
    let client_entity = Entity::create(&pool, owner.id, Some("e2e-test-client"), None, None, None)
        .await
        .expect("create client entity");

    let (server_a_algo, server_a_key_data) =
        generate_test_keypair(&server_a_key_path).expect("generate server A keypair");
    let (server_b_algo, server_b_key_data) =
        generate_test_keypair(&server_b_key_path).expect("generate server B keypair");
    let (client_algo, client_key_data) =
        generate_test_keypair(&client_key_path).expect("generate client keypair");

    SshKey::create(
        &pool,
        server_entity_a.id,
        &server_a_algo,
        &server_a_key_data,
        Some("e2e-test"),
        None,
        None,
    )
    .await
    .expect("register server A key");
    SshKey::create(
        &pool,
        server_entity_b.id,
        &server_b_algo,
        &server_b_key_data,
        Some("e2e-test"),
        None,
        None,
    )
    .await
    .expect("register server B key");
    SshKey::create(
        &pool,
        client_entity.id,
        &client_algo,
        &client_key_data,
        Some("e2e-test"),
        None,
        None,
    )
    .await
    .expect("register client key");

    // Client entity is granted access to both server entities specifically —
    // exercises the same `entity_access` check the SSH server enforces in
    // `channel_open_direct_tcpip`.
    EntityAccess::create(
        &pool,
        server_entity_a.id,
        "entity",
        Some(client_entity.id),
        None,
        None,
        None,
    )
    .await
    .expect("grant client access to server A");
    EntityAccess::create(
        &pool,
        server_entity_b.id,
        "entity",
        Some(client_entity.id),
        None,
        None,
        None,
    )
    .await
    .expect("grant client access to server B");

    // Local "real" services the tunnel is meant to reach — one per server
    // entity, each labeled so their responses are never confusable.
    let fixture_port_a = spawn_fixture_service("A").await;
    let fixture_port_b = spawn_fixture_service("B").await;

    // The t2t SSH rendezvous server itself, on an ephemeral port. The
    // `ActiveTunnels` map is shared with the test itself (mirroring how
    // `crates/t2t/src/main.rs` shares it with the web `AppState`) so this
    // test can directly assert on the live-connections tracking added
    // alongside `channel_open_direct_tcpip`, without standing up the full
    // HTTP API + session/login flow just to exercise the new route.
    let ssh_port = free_port();
    let ssh_pool = pool.clone();
    let ssh_host_key_path = host_key_path.to_str().unwrap().to_string();
    let server_slots = new_server_slots();
    let active_tunnels = new_active_tunnels();
    let test_active_tunnels = active_tunnels.clone();
    tokio::spawn(async move {
        start_ssh(
            SshConfig {
                ssh_port,
                fail2ban_log_path: None,
                host_key_path: ssh_host_key_path,
                host_key_password: None,
            },
            ssh_pool,
            server_slots,
            active_tunnels,
        )
        .await
        .expect("t2t SSH server failed");
    });

    // Give the listener a moment to come up before dialing it.
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", ssh_port)).await.is_ok() {
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }

    // Connections 1+2: both server entities register their own
    // `-R proxy_port:127.0.0.1:fixture_port` forward, each to its own
    // fixture service, at the same time.
    let _server_a_guard = spawn_ssh_r(
        &server_a_key_path,
        ssh_port,
        PROXY_PORT_A,
        fixture_port_a,
        "ssh -R (A)",
    );
    let _server_b_guard = spawn_ssh_r(
        &server_b_key_path,
        ssh_port,
        PROXY_PORT_B,
        fixture_port_b,
        "ssh -R (B)",
    );

    // `tcpip_forward` auto-creates a port_configs row the first time each
    // server registers its proxy_port — poll for both to appear, then
    // register the client's subscriptions. The subscription is what
    // `channel_open_direct_tcpip`'s step 2 requires now — a bare
    // `entity_access` grant is no longer sufficient on its own.
    let port_config_a = wait_for_port_config(&pool, server_entity_a.id, PROXY_PORT_A).await;
    let port_config_b = wait_for_port_config(&pool, server_entity_b.id, PROXY_PORT_B).await;

    let local_port_a = free_port();
    let local_port_b = free_port();
    PortSubscription::create(
        &pool,
        port_config_a.id,
        client_entity.id,
        local_port_a as i32,
        true,
    )
    .await
    .expect("create client subscription to server A's port_config");
    PortSubscription::create(
        &pool,
        port_config_b.id,
        client_entity.id,
        local_port_b as i32,
        true,
    )
    .await
    .expect("create client subscription to server B's port_config");

    // Connections 3+4: the "client" entity consumes both forwards via
    // `-L local_port:<server_entity_uuid>:proxy_port`. Using the entity's
    // UUID (not "localhost") as the forwarded address is exactly the case
    // that exposed the address-mismatch bug this test guards against.
    // Running both `-L` legs from the same client key/entity at the same
    // time is what actually exercises routing: it proves t2t picks the
    // right `(entity_id, proxy_port)` slot instead of e.g. the most
    // recently registered one.
    let _client_a_guard = spawn_ssh_l(
        &client_key_path,
        ssh_port,
        local_port_a,
        server_entity_a.id,
        PROXY_PORT_A,
        "ssh -L (A)",
    );
    let _client_b_guard = spawn_ssh_l(
        &client_key_path,
        ssh_port,
        local_port_b,
        server_entity_b.id,
        PROXY_PORT_B,
        "ssh -L (B)",
    );

    // First-contact: each tunnel must reach its own server and never the
    // other one's.
    let response_a = fetch_through_tunnel(
        local_port_a,
        "GET",
        "/",
        "",
        "hello from the tunneled service",
        Duration::from_secs(15),
    )
    .await
    .expect("traffic did not flow end-to-end through tunnel A");
    assert!(
        response_a.contains("A hello from the tunneled service"),
        "expected server A's fixture body in tunnel A's response, got: {response_a:?}"
    );
    let response_b = fetch_through_tunnel(
        local_port_b,
        "GET",
        "/",
        "",
        "hello from the tunneled service",
        Duration::from_secs(15),
    )
    .await
    .expect("traffic did not flow end-to-end through tunnel B");
    assert!(
        response_b.contains("B hello from the tunneled service"),
        "expected server B's fixture body in tunnel B's response, got: {response_b:?}"
    );
    assert_ne!(
        response_a, response_b,
        "tunnel A and tunnel B must never return each other's response"
    );

    // Multiple different requests through the same already-established
    // tunnel A: distinct paths, and a POST that round-trips a body — proves
    // request/response framing survives varied traffic, not just repeats of
    // one fixed GET.
    let alpha = fetch_through_tunnel(
        local_port_a,
        "GET",
        "/alpha",
        "",
        "alpha response body",
        Duration::from_secs(5),
    )
    .await
    .expect("GET /alpha through tunnel A failed");
    assert!(alpha.contains("A alpha response body"));

    let beta = fetch_through_tunnel(
        local_port_a,
        "GET",
        "/beta",
        "",
        "beta response body",
        Duration::from_secs(5),
    )
    .await
    .expect("GET /beta through tunnel A failed");
    assert!(beta.contains("A beta response body"));
    assert_ne!(alpha, beta, "distinct routes must yield distinct responses");

    let echo_payload = "line one\r\nline two\r\nthird line with a distinct payload";
    let echoed = fetch_through_tunnel(
        local_port_a,
        "POST",
        "/echo",
        echo_payload,
        echo_payload,
        Duration::from_secs(5),
    )
    .await
    .expect("POST /echo through tunnel A failed");
    assert!(
        echoed.contains(echo_payload),
        "expected the exact posted payload echoed back, got: {echoed:?}"
    );

    // Concurrent use of both tunnels at once: each must still only ever
    // return its own server's data.
    let (concurrent_a, concurrent_b) = tokio::join!(
        fetch_through_tunnel(
            local_port_a,
            "GET",
            "/alpha",
            "",
            "alpha response body",
            Duration::from_secs(5),
        ),
        fetch_through_tunnel(
            local_port_b,
            "GET",
            "/alpha",
            "",
            "alpha response body",
            Duration::from_secs(5),
        ),
    );
    assert!(concurrent_a
        .expect("concurrent GET /alpha through tunnel A failed")
        .contains("A alpha response body"));
    assert!(concurrent_b
        .expect("concurrent GET /alpha through tunnel B failed")
        .contains("B alpha response body"));

    // Phase 5: both bridges must be visible in the shared `ActiveTunnels`
    // map — this is exactly what `GET /api/entities/{id}/live-connections`
    // and `GET /api/admin/live-connections` read from. Every request fired
    // above already tore its own bridge back down by the time it returned
    // (the fixture service `shutdown()`s right after answering), so a fresh
    // connection is opened and deliberately left open here — that alone is
    // enough for `-L` to open its direct-tcpip channel, without needing any
    // HTTP traffic on it — to keep a bridge alive long enough to observe it.
    let _keep_alive_a = TcpStream::connect(("127.0.0.1", local_port_a))
        .await
        .expect("open keep-alive connection through tunnel A");
    let keep_alive_b = TcpStream::connect(("127.0.0.1", local_port_b))
        .await
        .expect("open keep-alive connection through tunnel B");

    let entry_a = wait_for_active_tunnel_entry(
        &test_active_tunnels,
        client_entity.id,
        server_entity_a.id,
        port_config_a.id,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(entry_a.proxy_port, PROXY_PORT_A as u32);
    assert_eq!(entry_a.client_user_id, owner.id);

    let entry_b = wait_for_active_tunnel_entry(
        &test_active_tunnels,
        client_entity.id,
        server_entity_b.id,
        port_config_b.id,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(entry_b.proxy_port, PROXY_PORT_B as u32);
    assert_eq!(entry_b.client_user_id, owner.id);

    // Tearing down the client leg's ssh process must remove that entry
    // again (both the explicit `channel_close` path and the `Drop`
    // fallback exist for this — killing the process here exercises
    // whichever one russh actually takes on an abrupt disconnect).
    drop(_client_a_guard);
    {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let still_present = test_active_tunnels
                .lock()
                .await
                .values()
                .any(|info| info.target_entity_id == server_entity_a.id);
            if !still_present {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "active_tunnels entry was never cleaned up after the client disconnected"
            );
            sleep(Duration::from_millis(150)).await;
        }
        // Tunnel B must be unaffected by tunnel A's client leg disconnecting.
        let still_has_b = test_active_tunnels
            .lock()
            .await
            .values()
            .any(|info| info.target_entity_id == server_entity_b.id);
        assert!(
            still_has_b,
            "tunnel B's active_tunnels entry must survive tunnel A's client disconnecting"
        );
        drop(keep_alive_b);
    }

    let _ = std::fs::remove_dir_all(&scratch);
}
