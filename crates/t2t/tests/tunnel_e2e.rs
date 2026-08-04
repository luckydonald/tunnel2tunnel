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

use std::sync::Arc;

use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::client::{connect_stream, Config as ClientConfig, Handle as ClientHandle};
use russh::keys::key::PrivateKeyWithHashAlg;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use russh::ChannelMsg;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::time::sleep;
use uuid::Uuid;

use tunnel2tunnel_core::{
    db,
    models::{
        connection_log::ConnectionLog, entity::Entity, entity_access::EntityAccess,
        port_config::PortConfig, port_subscription::PortSubscription, ssh_key::SshKey, user::User,
    },
    pubkey::parse_authorized_keys_line,
};
use tunnel2tunnel_ssh::{
    new_active_tunnels, new_live_update_tx, new_server_slots, start as start_ssh,
    ActiveTunnelInfo, ActiveTunnels, LiveUpdateTx, ServerSlots, SshConfig,
};
use tunnel2tunnel_web::{start as start_web, WebConfig};

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

/// Spawns `ssh -N` against the t2t server with no `-R`/`-L` request at all —
/// completes the SSH handshake and publickey auth (which is what flips
/// `entity_status` online via `ConnectionLog`), then just holds the
/// connection open. Used to reproduce "owner authenticated but hasn't
/// forwarded this port yet": unlike `spawn_ssh_r`, this never issues a
/// `tcpip_forward` request, so `server_slots` never gets an entry for this
/// entity/port — exactly the orange-ring scenario in
/// `crates/tunnel2tunnel-web/src/routes/live_connections.rs`.
fn spawn_ssh_authenticated_only(key_path: &Path, ssh_port: u16, label: &'static str) -> ChildGuard {
    let mut child = Command::new("ssh")
        .args(["-N"])
        .args(["-i", key_path.to_str().unwrap()])
        .args(["-p", &ssh_port.to_string()])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .arg("server@127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ssh -N (auth-only, no forward)");
    log_child_stderr(label, &mut child);
    ChildGuard(child)
}

/// Polls `ConnectionLog::entity_status` until `entity_id` shows up online
/// (a `connection_logs` row with `success = true AND ended_at IS NULL`),
/// or panics after 10s. This is exactly the signal SSH auth success writes,
/// independent of whether any port has been forwarded yet.
async fn wait_for_entity_online(pool: &sqlx::PgPool, entity_id: Uuid) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (online, _) = ConnectionLog::entity_status(pool, entity_id)
            .await
            .expect("query entity_status");
        if online {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "entity {entity_id} never showed up online in connection_logs"
        );
        sleep(Duration::from_millis(100)).await;
    }
}

/// Waits until `rx` observes at least one `live_update_tx` notification
/// (a `Lagged` counts too — it means several fired since the last check),
/// or panics after `timeout`. Regression guard for
/// `tunnel2tunnel-ssh::LiveUpdateTx` call sites silently going missing —
/// see that type's doc comment in `crates/tunnel2tunnel-ssh/src/lib.rs` for
/// the full list of state transitions expected to fire it.
async fn wait_for_live_update(rx: &mut tokio::sync::broadcast::Receiver<()>, timeout: Duration) {
    tokio::time::timeout(timeout, rx.recv())
        .await
        .expect("expected a live_update_tx notification")
        .ok(); // Ok(()) or Err(Lagged) both count as "it fired at least once"
}

/// Starts the real axum HTTP server (`tunnel2tunnel_web::start`) on a fresh
/// ephemeral port, sharing the same `pool`/`server_slots`/`active_tunnels`
/// the SSH server uses — mirrors how `crates/t2t/src/main.rs` wires both
/// together. Used so these tests exercise the actual
/// `GET /api/entities/{id}/live-connections` route (real session/login
/// flow, real JSON response) rather than re-deriving its logic locally.
async fn spawn_web_server(
    pool: sqlx::PgPool,
    server_slots: ServerSlots,
    active_tunnels: ActiveTunnels,
    live_update_tx: LiveUpdateTx,
) -> u16 {
    let http_port = free_port();
    tokio::spawn(async move {
        start_web(
            WebConfig {
                http_port,
                ssh_port: 0,
                static_dir: None,
                ssh_host_key_fingerprint: "test-fingerprint".to_string(),
            },
            pool,
            server_slots,
            active_tunnels,
            live_update_tx,
        )
        .await
        .expect("t2t web server failed");
    });
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", http_port)).await.is_ok() {
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    http_port
}

/// Logs into the given http server as `username`/`password` via the real
/// `POST /api/auth/login` route, returning a cookie-jar-enabled client that
/// carries the resulting session cookie on subsequent requests.
async fn login(http_port: u16, username: &str, password: &str) -> reqwest::Client {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .expect("build reqwest client");
    let resp = client
        .post(format!("http://127.0.0.1:{http_port}/api/auth/login"))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .expect("send login request");
    assert!(
        resp.status().is_success(),
        "login failed with status {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    client
}

/// Fetches `GET /api/entities/{entity_id}/live-connections` as an already
/// logged-in `client`, returning the parsed JSON body.
async fn get_entity_live_connections(
    client: &reqwest::Client,
    http_port: u16,
    entity_id: Uuid,
) -> serde_json::Value {
    let resp = client
        .get(format!(
            "http://127.0.0.1:{http_port}/api/entities/{entity_id}/live-connections"
        ))
        .send()
        .await
        .expect("send live-connections request");
    assert!(
        resp.status().is_success(),
        "live-connections request failed with status {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    resp.json().await.expect("parse live-connections JSON")
}

/// Finds the subscription row for `port_config_id` inside a
/// `GET /api/entities/{id}/live-connections` JSON body's `subscriptions`
/// array, panicking if it's missing.
fn find_subscription_row(body: &serde_json::Value, port_config_id: Uuid) -> serde_json::Value {
    body["subscriptions"]
        .as_array()
        .expect("subscriptions array")
        .iter()
        .find(|row| row["port_config_id"].as_str() == Some(&port_config_id.to_string()))
        .cloned()
        .unwrap_or_else(|| {
            panic!("no subscription row for port_config_id {port_config_id} in {body:?}")
        })
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
    // The password IS used now — the live-connections dashboard assertions
    // near the end of this test log in as `owner` via the real
    // `POST /api/auth/login` route.
    let owner_password = "e2e-test-owner-password-not-a-secret";
    let owner = User::create(
        &pool,
        &format!("t2t-e2e-test-{}", Uuid::now_v7()),
        None,
        owner_password,
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
    // `ActiveTunnels`/`ServerSlots` maps are shared with the test itself
    // (mirroring how `crates/t2t/src/main.rs` shares them with the web
    // `AppState`) so this test can both directly assert on the
    // live-connections tracking added alongside `channel_open_direct_tcpip`,
    // AND stand up the real HTTP API against the very same maps to exercise
    // `GET /api/entities/{id}/live-connections` end-to-end.
    let ssh_port = free_port();
    let ssh_pool = pool.clone();
    let ssh_host_key_path = host_key_path.to_str().unwrap().to_string();
    let server_slots = new_server_slots();
    let active_tunnels = new_active_tunnels();
    let live_update_tx = new_live_update_tx();
    let mut live_update_rx = live_update_tx.subscribe();
    let test_active_tunnels = active_tunnels.clone();
    let web_server_slots = server_slots.clone();
    let web_active_tunnels = active_tunnels.clone();
    let web_live_update_tx = live_update_tx.clone();
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
            live_update_tx,
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

    let http_port = spawn_web_server(
        pool.clone(),
        web_server_slots,
        web_active_tunnels,
        web_live_update_tx,
    )
    .await;
    let owner_client = login(http_port, &owner.username, owner_password).await;

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

    // `tcpip_forward` (both A and B just registered above) must fire
    // `live_update_tx` — this is what lets `routes::live_ws` push a fresh
    // snapshot to any connected socket the instant a port is forwarded.
    wait_for_live_update(&mut live_update_rx, Duration::from_secs(5)).await;

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

    // Both `channel_open_direct_tcpip` bridges above must also have fired
    // `live_update_tx` (at least the fast-path insert did, twice).
    wait_for_live_update(&mut live_update_rx, Duration::from_secs(5)).await;

    // Fully-bridged live-connections check: with both `-R` forwards
    // registered AND a live subscriber channel open on each, the client
    // entity's subscription rows must report `live: true` (its own bridge is
    // up) and `remote_status: "active"` (an active bridge exists right now)
    // — the fixed happy path this bug's not_forwarded/idle distinction is
    // contrasted against below.
    let client_live_connections =
        get_entity_live_connections(&owner_client, http_port, client_entity.id).await;
    let sub_row_a = find_subscription_row(&client_live_connections, port_config_a.id);
    assert_eq!(sub_row_a["live"], serde_json::json!(true));
    assert_eq!(sub_row_a["remote_status"], serde_json::json!("active"));
    let sub_row_b = find_subscription_row(&client_live_connections, port_config_b.id);
    assert_eq!(sub_row_b["live"], serde_json::json!(true));
    assert_eq!(sub_row_b["remote_status"], serde_json::json!("active"));

    // The service (owner) side must also aggregate `live: true` now that a
    // subscriber has an active bridge, and now carries its own
    // `remote_status` ring too (mirroring the subscriber side's semantics,
    // just from this entity's own point of view) — `"active"` here as well,
    // since a bridge is up.
    let server_a_live_connections =
        get_entity_live_connections(&owner_client, http_port, server_entity_a.id).await;
    let service_row_a = server_a_live_connections["services"]
        .as_array()
        .expect("services array")
        .iter()
        .find(|row| row["port_config_id"].as_str() == Some(&port_config_a.id.to_string()))
        .unwrap_or_else(|| {
            panic!("no service row for port_config_a in {server_a_live_connections:?}")
        });
    assert_eq!(service_row_a["live"], serde_json::json!(true));
    assert_eq!(service_row_a["remote_status"], serde_json::json!("active"));

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

        // The `channel_close`/`Drop` cleanup that just removed tunnel A's
        // `active_tunnels` entry must also have fired `live_update_tx`.
        wait_for_live_update(&mut live_update_rx, Duration::from_secs(5)).await;
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

        // Tunnel A's subscriber row must now report `live: false` — no
        // bridge left for this subscriber — and `remote_status` drops from
        // `"active"` to `"idle"`: server A is still SSH-connected
        // (`_server_a_guard` is still alive) and its `-R` forward is still
        // registered, but with no bridge open the ring now reflects "port
        // forwarded/routable but idle" rather than "actively bridged".
        let client_live_connections_after_a_disconnect =
            get_entity_live_connections(&owner_client, http_port, client_entity.id).await;
        let sub_row_a_after = find_subscription_row(&client_live_connections_after_a_disconnect, port_config_a.id);
        assert_eq!(sub_row_a_after["live"], serde_json::json!(false));
        assert_eq!(sub_row_a_after["remote_status"], serde_json::json!("idle"));

        drop(keep_alive_b);
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

// ── Regression test: one connection, one offline target, one online target ──

/// Minimal in-process SSH client `Handler` — accepts any host key, mirroring
/// `tarpit_e2e.rs`'s `TestClient`. Needed for the test below (rather than
/// driving real `ssh` subprocesses like the rest of this file) because it
/// must issue two `channel_open_direct_tcpip` requests concurrently on the
/// SAME connection/`Handle` — impossible with separate OS `ssh` processes,
/// which always open their own independent connection.
struct DirectTcpipTestClient;

impl russh::client::Handler for DirectTcpipTestClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn connect_direct_tcpip_test_client(port: u16) -> ClientHandle<DirectTcpipTestClient> {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to t2t server");
    connect_stream(
        Arc::new(ClientConfig::default()),
        stream,
        DirectTcpipTestClient,
    )
    .await
    .expect("client connect")
}

/// Generates a fresh Ed25519 keypair kept entirely in memory — unlike
/// `generate_test_keypair`, which writes to a file for `ssh -i`. Mirrors
/// `tarpit_e2e.rs`'s `generate_keypair`.
fn generate_inprocess_keypair() -> (PrivateKey, String, String) {
    let key =
        PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519).expect("generate keypair");
    let pub_line = key.public_key().to_openssh().expect("openssh pub line");
    let (algorithm, key_data, _comment) =
        parse_authorized_keys_line(&pub_line).expect("parse pub line");
    (key, algorithm, key_data)
}

/// Regression test for a bug where `channel_open_direct_tcpip`
/// (`crates/tunnel2tunnel-ssh/src/lib.rs`) ran its up-to-12s server-liveness
/// retry-wait loop inline inside the `Handler` callback. Since russh
/// processes one SSH connection on a single per-connection task
/// (`server/session.rs`'s `select!` loop), that inline wait blocked ALL
/// other channel activity on the same connection — including an unrelated,
/// perfectly healthy tunnel's channel-open request that happened to share
/// the connection. Observed in production as a VNC `-L` tunnel to a live
/// target getting stuck on "Connecting..." because a second `-L` on the
/// same `ssh` invocation, to a target that never came online, ate the full
/// 12s retry window first.
///
/// `two_ssh_connections_tunnel_through_rendezvous` above never caught this:
/// its concurrency is across two independent `ssh` subprocesses (= two
/// independent connections), which structurally cannot exercise
/// same-connection serialization. This test drives ONE connection via an
/// in-process `russh::client::Handle` (cheaply `Clone`) instead, so it can
/// fire both requests through the same `Handle` and observe whether one
/// blocks behind the other.
#[tokio::test]
async fn same_connection_online_target_not_blocked_by_offline_target_timeout() {
    if Command::new("ssh").arg("-V").output().await.is_err() {
        eprintln!(
            "skipping same_connection_online_target_not_blocked_by_offline_target_timeout: \
             no `ssh` binary in PATH"
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

    let scratch = std::env::temp_dir().join(format!("t2t-e2e-blocking-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let host_key_path = scratch.join("ssh_host_key");
    let server_online_key_path = scratch.join("server_online_key");

    // The password IS used — the gray-ring live-connections assertion near
    // the end of this test logs in as `owner` via the real
    // `POST /api/auth/login` route.
    let owner_password = "e2e-test-blocking-owner-password-not-a-secret";
    let owner = User::create(
        &pool,
        &format!("t2t-e2e-blocking-test-{}", Uuid::now_v7()),
        None,
        owner_password,
        false,
        Some("scratch user for tunnel_e2e blocking-bug regression test"),
    )
    .await
    .expect("create test user");

    let server_entity_online = Entity::create(
        &pool,
        owner.id,
        Some("blocking-test-server-online"),
        None,
        None,
        None,
    )
    .await
    .expect("create online server entity");
    let server_entity_offline = Entity::create(
        &pool,
        owner.id,
        Some("blocking-test-server-offline"),
        None,
        None,
        None,
    )
    .await
    .expect("create offline server entity");
    let client_entity = Entity::create(
        &pool,
        owner.id,
        Some("blocking-test-client"),
        None,
        None,
        None,
    )
    .await
    .expect("create client entity");

    let (server_online_algo, server_online_key_data) =
        generate_test_keypair(&server_online_key_path).expect("generate online server keypair");
    let (client_key, client_algo, client_key_data) = generate_inprocess_keypair();

    SshKey::create(
        &pool,
        server_entity_online.id,
        &server_online_algo,
        &server_online_key_data,
        Some("e2e-test"),
        None,
        None,
    )
    .await
    .expect("register online server key");
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

    EntityAccess::create(
        &pool,
        server_entity_online.id,
        "entity",
        Some(client_entity.id),
        None,
        None,
        None,
    )
    .await
    .expect("grant client access to online server");
    EntityAccess::create(
        &pool,
        server_entity_offline.id,
        "entity",
        Some(client_entity.id),
        None,
        None,
        None,
    )
    .await
    .expect("grant client access to offline server");

    let fixture_port = spawn_fixture_service("A").await;

    let ssh_port = free_port();
    let ssh_pool = pool.clone();
    let ssh_host_key_path = host_key_path.to_str().unwrap().to_string();
    let server_slots = new_server_slots();
    let active_tunnels = new_active_tunnels();
    let live_update_tx = new_live_update_tx();
    let web_server_slots = server_slots.clone();
    let web_active_tunnels = active_tunnels.clone();
    let web_live_update_tx = live_update_tx.clone();
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
            live_update_tx,
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

    let http_port = spawn_web_server(
        pool.clone(),
        web_server_slots,
        web_active_tunnels,
        web_live_update_tx,
    )
    .await;
    let owner_client = login(http_port, &owner.username, owner_password).await;

    // Only the online target ever registers a `-R` forward — the offline
    // one deliberately never does, exercising "target server never came
    // online within the retry window" (lib.rs's `channel_open_direct_tcpip`)
    // on demand rather than relying on a race.
    const PROXY_PORT_ONLINE: u16 = 19223;
    const PROXY_PORT_OFFLINE: u16 = 19224;
    let _server_online_guard = spawn_ssh_r(
        &server_online_key_path,
        ssh_port,
        PROXY_PORT_ONLINE,
        fixture_port,
        "ssh -R (blocking-test online)",
    );

    let port_config_online =
        wait_for_port_config(&pool, server_entity_online.id, PROXY_PORT_ONLINE).await;
    // The offline target's port_config is created directly — no `-R` ever
    // registers it, so `tcpip_forward`'s auto-create path never runs for it.
    let port_config_offline = PortConfig::create(
        &pool,
        server_entity_offline.id,
        true,
        PROXY_PORT_OFFLINE as i32,
        PROXY_PORT_OFFLINE as i32,
        "blocking-test-offline",
        None,
        0,
        "127.0.0.1",
    )
    .await
    .expect("create offline target's port_config directly");

    PortSubscription::create(
        &pool,
        port_config_online.id,
        client_entity.id,
        PROXY_PORT_ONLINE as i32,
        true,
    )
    .await
    .expect("subscribe client to online target");
    PortSubscription::create(
        &pool,
        port_config_offline.id,
        client_entity.id,
        PROXY_PORT_OFFLINE as i32,
        true,
    )
    .await
    .expect("subscribe client to offline target");

    // One shared connection/`Handle` for BOTH requests — the crux of this
    // test. `channel_open_direct_tcpip` takes `&self` and uses its own
    // per-call reply channel internally (not `Handle`'s own auth-reply
    // receiver), so concurrent calls on one `Handle` are safe without
    // needing `&mut` — wrap in `Arc` just to share ownership across the two
    // spawned tasks below.
    let mut handle = connect_direct_tcpip_test_client(ssh_port).await;
    let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(client_key), None);
    let auth_result = handle
        .authenticate_publickey("client", key_with_alg)
        .await
        .expect("authenticate_publickey");
    assert!(auth_result.success(), "client publickey auth must succeed");
    let handle = Arc::new(handle);

    // Fire both requests as independent tasks sharing the same `handle`, so
    // awaiting one does not itself stop the other from being polled — this
    // is what actually puts two channel-open requests in flight on one
    // connection at once.
    let online_handle = handle.clone();
    let online_task = tokio::spawn(async move {
        let started = tokio::time::Instant::now();
        let result = online_handle
            .channel_open_direct_tcpip(
                server_entity_online.id.to_string(),
                PROXY_PORT_ONLINE as u32,
                "127.0.0.1",
                0,
            )
            .await;
        (result, started.elapsed())
    });

    let offline_handle = handle.clone();
    let offline_task = tokio::spawn(async move {
        offline_handle
            .channel_open_direct_tcpip(
                server_entity_offline.id.to_string(),
                PROXY_PORT_OFFLINE as u32,
                "127.0.0.1",
                0,
            )
            .await
    });

    let (online_result, online_elapsed) = tokio::time::timeout(Duration::from_secs(5), online_task)
        .await
        .expect(
            "the ONLINE target's channel-open must resolve well under the offline target's 12s \
             retry window — if this times out, the two requests are being serialized on the \
             shared connection (the bug this test guards against)",
        )
        .expect("online task panicked");
    let mut online_channel =
        online_result.expect("channel_open_direct_tcpip to the online target must succeed");
    assert!(
        online_elapsed < Duration::from_secs(3),
        "online target's channel took {online_elapsed:?} to confirm — expected it to not be \
         blocked behind the offline target's 12s retry window"
    );

    // Prove the bridge is actually live end-to-end, not merely confirmed —
    // send a real request and expect the fixture service's response back.
    online_channel
        .data_bytes(
            b"GET / HTTP/1.1\r\nHost: t2t-test\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
        .await
        .expect("write through the online bridge");
    let msg = tokio::time::timeout(Duration::from_secs(5), online_channel.wait())
        .await
        .expect("expected a response through the online bridge")
        .expect("channel closed with no data");
    let ChannelMsg::Data { data } = msg else {
        panic!("expected response Data through the online bridge, got: {msg:?}");
    };
    assert!(
        String::from_utf8_lossy(&data).contains("A hello from the tunneled service"),
        "expected the online target's fixture response, got: {:?}",
        String::from_utf8_lossy(&data)
    );

    // The offline target's channel is confirmed too (steps 1-3 of
    // `channel_open_direct_tcpip` pass — auth/port_config/subscription are
    // all valid), then closed by the spawned retry-wait task once its 12s
    // deadline lapses ("confirm-then-close-on-timeout", the documented
    // trade-off of not blocking the connection to keep the retry window).
    // Prove it actually closes rather than staying open forever.
    let offline_result = tokio::time::timeout(Duration::from_secs(5), offline_task)
        .await
        .expect("offline task must not be blocked either")
        .expect("offline task panicked");
    let mut offline_channel = offline_result
        .expect("channel_open_direct_tcpip to the offline target should still be confirmed");
    let offline_msg = tokio::time::timeout(Duration::from_secs(14), offline_channel.wait())
        .await
        .expect("expected the offline target's channel to close once the 12s retry window lapses");
    assert!(
        matches!(
            offline_msg,
            None | Some(ChannelMsg::Close) | Some(ChannelMsg::Eof)
        ),
        "expected the offline target's channel to close/eof after the retry window elapsed, \
         got: {offline_msg:?}"
    );

    // Live-connections offline-ring regression check: `server_entity_offline`
    // never registers an SSH key and never connects at all (unlike the
    // not_forwarded scenario in `subscriber_sees_not_forwarded_ring_when_owner_online_but_port_not_forwarded`,
    // where the owner IS authenticated but just hasn't forwarded yet) — its
    // subscriber row must show `live: false, remote_status: "offline"`. The
    // online target's row, by contrast, must show `remote_status: "active"`
    // (SSH-connected, the port is forwarded, AND its bridge is still open
    // from the request above) even though this particular client used an
    // in-process `russh::client::Handle` rather than a real `ssh -L`
    // subprocess.
    let client_live_connections =
        get_entity_live_connections(&owner_client, http_port, client_entity.id).await;
    let offline_row = find_subscription_row(&client_live_connections, port_config_offline.id);
    assert_eq!(offline_row["live"], serde_json::json!(false));
    assert_eq!(offline_row["remote_status"], serde_json::json!("offline"));
    let online_row = find_subscription_row(&client_live_connections, port_config_online.id);
    assert_eq!(online_row["remote_status"], serde_json::json!("active"));

    let _ = std::fs::remove_dir_all(&scratch);
}

// ── Regression test: owner authenticated over SSH, but hasn't forwarded yet ──

/// Regression test for the exact bug scenario documented in
/// `crates/tunnel2tunnel-web/src/routes/live_connections.rs`'s module doc
/// comment: an owner entity completes SSH publickey auth (so
/// `ConnectionLog::entity_status` reports it online) but hasn't (yet, or
/// ever) issued a `tcpip_forward` request for a given port, so `server_slots`
/// has no entry for `(owner_entity_id, proxy_port)`. Before the original fix,
/// a subscriber's port dot just stayed gray in this case — indistinguishable
/// from the owner being fully offline. This test drives a real `ssh -N`
/// connection with no `-R` at all (see `spawn_ssh_authenticated_only`) to
/// reach that exact state, then asserts the subscriber's live-connections
/// row via the real `GET /api/entities/{id}/live-connections` route reports
/// `live: false, remote_status: "not_forwarded"`.
#[tokio::test]
async fn subscriber_sees_not_forwarded_ring_when_owner_online_but_port_not_forwarded() {
    if Command::new("ssh").arg("-V").output().await.is_err() {
        eprintln!(
            "skipping subscriber_sees_not_forwarded_ring_when_owner_online_but_port_not_forwarded: \
             no `ssh` binary in PATH"
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

    let scratch = std::env::temp_dir().join(format!("t2t-e2e-orange-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let host_key_path = scratch.join("ssh_host_key");
    let owner_key_path = scratch.join("owner_key");

    // The password IS used — logging in as `owner` to hit the
    // live-connections route below via the real `POST /api/auth/login`.
    let owner_password = "e2e-test-orange-owner-password-not-a-secret";
    let owner = User::create(
        &pool,
        &format!("t2t-e2e-orange-test-{}", Uuid::now_v7()),
        None,
        owner_password,
        false,
        Some("scratch user for tunnel_e2e orange-ring regression test"),
    )
    .await
    .expect("create test user");

    // Both entities belong to the same user: `owner_entity` is the service
    // side that authenticates but never forwards; `client_entity` is the
    // subscriber whose live-connections row is asserted on.
    let owner_entity = Entity::create(
        &pool,
        owner.id,
        Some("orange-test-owner"),
        None,
        None,
        None,
    )
    .await
    .expect("create owner entity");
    let client_entity = Entity::create(
        &pool,
        owner.id,
        Some("orange-test-client"),
        None,
        None,
        None,
    )
    .await
    .expect("create client entity");

    let (owner_algo, owner_key_data) =
        generate_test_keypair(&owner_key_path).expect("generate owner keypair");
    SshKey::create(
        &pool,
        owner_entity.id,
        &owner_algo,
        &owner_key_data,
        Some("e2e-test"),
        None,
        None,
    )
    .await
    .expect("register owner key");

    // No `-R` ever registers this port_config, so `tcpip_forward`'s
    // auto-create path never runs for it — created directly, same as the
    // offline target in `same_connection_online_target_not_blocked_by_offline_target_timeout`.
    const PROXY_PORT_NOT_FORWARDED: u16 = 19323;
    let port_config = PortConfig::create(
        &pool,
        owner_entity.id,
        true,
        PROXY_PORT_NOT_FORWARDED as i32,
        PROXY_PORT_NOT_FORWARDED as i32,
        "orange-test-not-forwarded",
        None,
        0,
        "127.0.0.1",
    )
    .await
    .expect("create owner's port_config directly");
    PortSubscription::create(
        &pool,
        port_config.id,
        client_entity.id,
        PROXY_PORT_NOT_FORWARDED as i32,
        true,
    )
    .await
    .expect("subscribe client to owner's port_config");

    let ssh_port = free_port();
    let ssh_pool = pool.clone();
    let ssh_host_key_path = host_key_path.to_str().unwrap().to_string();
    let server_slots = new_server_slots();
    let active_tunnels = new_active_tunnels();
    let live_update_tx = new_live_update_tx();
    let test_server_slots = server_slots.clone();
    let web_server_slots = server_slots.clone();
    let web_active_tunnels = active_tunnels.clone();
    let web_live_update_tx = live_update_tx.clone();
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
            live_update_tx,
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

    let http_port = spawn_web_server(
        pool.clone(),
        web_server_slots,
        web_active_tunnels,
        web_live_update_tx,
    )
    .await;
    let owner_client = login(http_port, &owner.username, owner_password).await;

    // The owner authenticates and holds the connection open, but never
    // issues a `-R` forward request for `PROXY_PORT_NOT_FORWARDED` (or any
    // port at all) — this is the crux of the regression this test guards
    // against.
    let _owner_guard = spawn_ssh_authenticated_only(&owner_key_path, ssh_port, "ssh -N (orange)");

    wait_for_entity_online(&pool, owner_entity.id).await;

    // Confirm the exact precondition the bug hinges on: online, but no
    // `server_slots` entry for this entity/port — before trusting the
    // route's JSON response to reflect it correctly.
    assert!(
        !test_server_slots
            .lock()
            .await
            .contains_key(&(owner_entity.id, PROXY_PORT_NOT_FORWARDED as u32)),
        "owner_entity must have no server_slots entry — it never issued a `-R` forward"
    );

    let client_live_connections =
        get_entity_live_connections(&owner_client, http_port, client_entity.id).await;
    let sub_row = find_subscription_row(&client_live_connections, port_config.id);
    assert_eq!(
        sub_row["live"],
        serde_json::json!(false),
        "expected live: false for a subscriber with no active bridge, got: {sub_row:?}"
    );
    assert_eq!(
        sub_row["remote_status"],
        serde_json::json!("not_forwarded"),
        "expected remote_status: \"not_forwarded\" (owner online, port not forwarded), got: {sub_row:?}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
