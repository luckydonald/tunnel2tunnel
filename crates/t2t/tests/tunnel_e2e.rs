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
    models::{entity::Entity, entity_access::EntityAccess, ssh_key::SshKey, user::User},
    pubkey::parse_authorized_keys_line,
};
use tunnel2tunnel_ssh::{start as start_ssh, SshConfig};

const FIXTURE_BODY: &str = "hello from the tunneled service";
/// The remote-forward routing key used between the two `ssh` legs. This is
/// never bound as a real OS socket by t2t (see `tcpip_forward` in
/// `tunnel2tunnel-ssh/src/lib.rs`) — it's purely the map key t2t uses to
/// pair a `-L` request with the matching `-R` registration, so a fixed
/// constant is safe even under parallel test runs.
const PROXY_PORT: u16 = 19123;

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

/// Local fixture "service" that the server entity forwards to: replies with
/// a fixed HTTP response on every connection. Runs for the test's lifetime.
async fn spawn_fixture_service() -> u16 {
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
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await; // drain the request, contents unused
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    FIXTURE_BODY.len(),
                    FIXTURE_BODY
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    port
}

/// Repeatedly connects to `port` and issues a bare HTTP GET, retrying until
/// the fixture body comes back or `timeout` elapses. A fresh connection is
/// used per attempt because a rejected direct-tcpip channel closes whatever
/// local connection triggered it, without killing the `ssh -L` process
/// itself — so early attempts made before the `-R` side has registered are
/// expected to fail and should just be retried.
async fn fetch_through_tunnel(port: u16, timeout: Duration) -> anyhow::Result<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let attempt = async {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
            stream
                .write_all(b"GET / HTTP/1.1\r\nHost: t2t-test\r\nConnection: close\r\n\r\n")
                .await?;
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await?;
            anyhow::Ok(String::from_utf8_lossy(&buf).into_owned())
        }
        .await;

        match attempt {
            Ok(response) if response.contains(FIXTURE_BODY) => return Ok(response),
            _ if tokio::time::Instant::now() < deadline => sleep(Duration::from_millis(150)).await,
            Ok(response) => {
                anyhow::bail!("tunnel never returned the fixture body; last response: {response:?}")
            }
            Err(e) => anyhow::bail!("tunnel never became reachable: {e}"),
        }
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
    let server_key_path = scratch.join("server_key");
    let client_key_path = scratch.join("client_key");

    // Two entities, owned by a fresh dedicated user so repeated runs never
    // collide with each other or with unrelated data in the same database.
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

    let server_entity = Entity::create(
        &pool,
        owner.id,
        "server",
        Some("e2e-test-server"),
        None,
        None,
        None,
    )
    .await
    .expect("create server entity");
    let client_entity = Entity::create(
        &pool,
        owner.id,
        "client",
        Some("e2e-test-client"),
        None,
        None,
        None,
    )
    .await
    .expect("create client entity");

    let (server_algo, server_key_data) =
        generate_test_keypair(&server_key_path).expect("generate server keypair");
    let (client_algo, client_key_data) =
        generate_test_keypair(&client_key_path).expect("generate client keypair");

    SshKey::create(
        &pool,
        server_entity.id,
        &server_algo,
        &server_key_data,
        Some("e2e-test"),
        None,
        None,
    )
    .await
    .expect("register server key");
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

    // Client entity is granted access to the server entity specifically —
    // exercises the same `entity_access` check the SSH server enforces in
    // `channel_open_direct_tcpip`.
    EntityAccess::create(
        &pool,
        server_entity.id,
        "entity",
        Some(client_entity.id),
        None,
        None,
    )
    .await
    .expect("grant client access to server");

    // Local "real" service the tunnel is meant to reach.
    let fixture_port = spawn_fixture_service().await;

    // The t2t SSH rendezvous server itself, on an ephemeral port.
    let ssh_port = free_port();
    let ssh_pool = pool.clone();
    let ssh_host_key_path = host_key_path.to_str().unwrap().to_string();
    tokio::spawn(async move {
        start_ssh(
            SshConfig {
                ssh_port,
                fail2ban_log_path: None,
                host_key_path: ssh_host_key_path,
                host_key_password: None,
            },
            ssh_pool,
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

    // Connection 1: the "server" entity registers `-R proxy_port:127.0.0.1:fixture_port`.
    let mut server_ssh = Command::new("ssh")
        .args([
            "-N",
            "-R",
            &format!("{PROXY_PORT}:127.0.0.1:{fixture_port}"),
        ])
        .args(["-i", server_key_path.to_str().unwrap()])
        .args(["-p", &ssh_port.to_string()])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .arg("server@127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ssh -R");
    log_child_stderr("ssh -R", &mut server_ssh);
    let _server_guard = ChildGuard(server_ssh);

    // Connection 2: the "client" entity consumes it via
    // `-L local_port:<server_entity_uuid>:proxy_port`. Using the entity's
    // UUID (not "localhost") as the forwarded address is exactly the case
    // that exposed the address-mismatch bug this test guards against.
    let local_port = free_port();
    let mut client_ssh = Command::new("ssh")
        .args([
            "-N",
            "-L",
            &format!("{local_port}:{}:{PROXY_PORT}", server_entity.id),
        ])
        .args(["-i", client_key_path.to_str().unwrap()])
        .args(["-p", &ssh_port.to_string()])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .arg("client@127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ssh -L");
    log_child_stderr("ssh -L", &mut client_ssh);
    let _client_guard = ChildGuard(client_ssh);

    let response = fetch_through_tunnel(local_port, Duration::from_secs(15))
        .await
        .expect("traffic did not flow end-to-end through the tunnel");
    assert!(
        response.contains(FIXTURE_BODY),
        "expected fixture body in tunneled response, got: {response:?}"
    );

    // A second fetch confirms both ssh connections stay alive across
    // repeated use, matching the manual test's verification step.
    let response2 = fetch_through_tunnel(local_port, Duration::from_secs(5))
        .await
        .expect("second request through the tunnel failed");
    assert!(response2.contains(FIXTURE_BODY));

    let _ = std::fs::remove_dir_all(&scratch);
}
