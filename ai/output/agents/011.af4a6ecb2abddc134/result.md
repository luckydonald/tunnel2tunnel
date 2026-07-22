I now have everything needed. Here's my report.

## 1. fail2ban integration

**Filter file** — `/home/user/git/luckydonald/tunnel2tunnel/contrib/fail2ban/filter.d/tunnel2tunnel.conf`:
```
[Definition]
# Matches lines written by tunnel2tunnel to FAIL2BAN_LOG_PATH
# Format mirrors sshd: "... Failed publickey for invalid user <fp> from <IP> port <p> ssh2"
failregex = ^.+ t2t sshd\[\d+\]: Failed publickey for invalid user .+ from <HOST> port \d+ ssh2$
ignoreregex =
```
Note the filter only matches `Failed publickey ... invalid user`, not the `Accepted publickey` success line (which the Rust code also writes but the filter ignores).

**CLAUDE.md** documents it at line 136 (env var table) and line 188:
- `crates/tunnel2tunnel-ssh/CLAUDE.md`/root `CLAUDE.md:136`: `| FAIL2BAN_LOG_PATH | — | If set, write sshd-format auth events to this file |`
- `CLAUDE.md:188`: `fail2ban log format: <Month> <day> <time> t2t sshd[0]: Failed/Accepted publickey for …`

**Rust code** in `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs`:

- `SshConfig.fail2ban_log_path: Option<String>` (line 31), converted to `Arc<String>` and stored on both `T2tServer.fail2ban` (line 115) and cloned per-connection into `T2tHandler.fail2ban` (line 155, set at line 131 in `new_client`).

- The two writer methods, both `&self`/`&mut self` methods on `T2tHandler`:

```rust
// lines 163-197
async fn log_auth_failure(
    &self,
    fingerprint: Option<&str>,
    reason: &str,
) {
    ...
    if let Some(path) = &self.fail2ban {
        let fp = fingerprint.unwrap_or("unknown");
        let line = format!(
            "{} t2t sshd[0]: Failed publickey for invalid user {} from {} port 0 ssh2\n",
            chrono_like_timestamp(),
            fp,
            self.peer_ip,
        );
        if let Err(e) = append_to_file(path, &line).await {
            tracing::warn!(err = %e, "fail2ban write failed");
        }
    }
}
```

```rust
// lines 199-228
async fn log_auth_success(&mut self, entity: &Entity, fingerprint: &str) {
    ...
    if let Some(path) = &self.fail2ban {
        let line = format!(
            "{} t2t sshd[0]: Accepted publickey for {} from {} port 0 ssh2\n",
            chrono_like_timestamp(),
            entity.id,
            self.peer_ip,
        );
        if let Err(e) = append_to_file(path, &line).await {
            tracing::warn!(err = %e, "fail2ban write failed");
        }
    }
}
```

- Shared low-level helpers used by both (lines 917-940):

```rust
fn chrono_like_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    let month = months[now.month() as usize - 1];
    format!(
        "{} {:2} {:02}:{:02}:{:02}",
        month, now.day(), now.hour(), now.minute(), now.second(),
    )
}

async fn append_to_file(path: &str, content: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    Ok(())
}
```

- **Callers in the auth handler** (`impl Handler for T2tHandler`, `auth_publickey` / `auth_password`):
  - `auth_password` (line 247) → always rejects → `self.log_auth_failure(None, "password auth not supported").await;` (line 255)
  - `auth_publickey` (line 298) calls `log_auth_failure` at each rejection path: unknown key (315), key expired soft-delete (330), key expired valid_until (338), entity not found (348), entity expired (363), IP blocked (377) — each passing `Some(&fp)` and a distinct `reason: &str`.
  - On success (line 388-391): `let user_id = entity.user_id; self.log_auth_success(&entity, &fp).await; self.entity = Some(AuthedEntity { entity, user_id });`

For a tarpit event, the natural approach is to add a third method (e.g. `log_tarpit_trigger`) following the exact same pattern: build the sshd-mirrored `format!` line with `chrono_like_timestamp()`, guard on `if let Some(path) = &self.fail2ban`, and call `append_to_file(path, &line).await`, then call it from wherever the tarpit logic lives in the auth flow. Note: `log_auth_failure` takes `&self` (immutable), so it can be called from any auth callback without extra `&mut self` plumbing.

## 2. Existing test patterns

Only one test file exists in the whole workspace with async server-integration tests: `/home/user/git/luckydonald/tunnel2tunnel/crates/t2t/tests/tunnel_e2e.rs` (297 lines, single `#[tokio::test]`: `two_ssh_connections_tunnel_through_rendezvous`, lines 151-297).

Other `#[test]` locations are plain unit tests, not server-integration:
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/pubkey.rs` lines 36-70ish (`mod tests`, `parse_authorized_keys_line_with_comment`, `fingerprint_stable`, etc.)
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/ip_whitelist.rs` lines 77-121 (`mod tests`, `evaluate()` cases: `empty_whitelist_allows_all`, `plain_ip_match`, `cidr_match`, `glob_match`, `regex_match`, `inversion_deny`, `no_match_denies`)

**Important finding: no russh client API is used anywhere in the repo.** `grep` for `russh::client` and `use russh` across all crates shows only `russh::server`/`russh::keys`/`russh::keys::ssh_key` imports in `tunnel2tunnel-ssh/src/lib.rs`, `t2t/tests/tunnel_e2e.rs`, and `t2t-feasibility/src/main.rs` — never `russh::client`. There is no in-process russh client harness to imitate; instead the existing e2e test drives the server via **real OpenSSH client subprocesses** (`Command::new("ssh")`), explicitly by design (see doc comment lines 1-10: "A real `ssh` client binary is used for both legs (not a Rust SSH client) because this is exactly what caught the forwarded-tcpip address bug... only a real OpenSSH client reproduces that matching behavior"). If you want a new test simulating legit-login vs. tarpit-triggering sequences, you have two realistic options based on this codebase's conventions:
1. Follow the same subprocess pattern (spawn `ssh -i key ...` and check exit/behavior), matching existing style, or
2. Write a `russh::client`-based test (new dependency usage, not yet present) if you need finer control over raw auth method sequencing (e.g. multiple failed pubkey offers to trigger a tarpit threshold) that OpenSSH's client won't let you script easily.

**Fixture/setup helpers reusable for a new test** (all in `tunnel_e2e.rs`):
- DB bootstrap (lines 158-167):
```rust
let database_url = std::env::var("DATABASE_URL")
    .unwrap_or_else(|_| "postgres://t2t:t2t_secret@localhost:5432/tunnel2tunnel".to_string());
let pool = db::connect(&database_url).await.expect(...);
sqlx::migrate!("../../migrations").run(&pool).await.expect("failed to run migrations");
```
- Scratch dir for keys, per-run unique via `Uuid::now_v7()` (lines 169-175), cleaned up at the end (line 296: `std::fs::remove_dir_all(&scratch)`).
- Test user/entities/keys/access grants via `User::create`, `Entity::create`, `SshKey::create`, `EntityAccess::create` (lines 179-214) — same core model APIs the SSH handler itself uses.
- `generate_test_keypair(priv_path: &Path) -> anyhow::Result<(String, String)>` (lines 79-94) — generates an Ed25519 keypair, writes private key to disk (mode 0600), returns `(algorithm, key_data_b64)` for `SshKey::create`.
- `free_port()` (lines 48-54) — grabs an ephemeral OS port by binding then immediately dropping a `TcpListener`.
- Starting the server under test (lines 219-235):
```rust
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
// poll until listening
for _ in 0..50 {
    if TcpStream::connect(("127.0.0.1", ssh_port)).await.is_ok() { break; }
    sleep(Duration::from_millis(100)).await;
}
```
  Note `fail2ban_log_path: None` here — a new tarpit test would set this to `Some(scratch.join("fail2ban.log")...)` and then read the file back to assert on written lines, following the same `append_to_file` format.
- `ChildGuard` (lines 58-64) — wraps a spawned `ssh` child process and kills it on `Drop`, so a panicking assertion never leaks a process.
- `log_child_stderr` (lines 68-77) — streams a child's stderr to the test's stderr with a name prefix, non-blocking.
- Spawning the actual `ssh` subprocess for a login attempt (lines 246-257, 266-279) — pattern to copy for legit-login and tarpit-triggering (e.g. repeated bad-key) sequences, using `-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null`.
- `fetch_through_tunnel` (lines 130-149) — retry-loop helper to poll a local port until the tunneled fixture response appears, useful as a model for a "poll until fail2ban log contains expected line" helper.

No test currently touches the `FAIL2BAN_LOG_PATH` mechanism at all — `tunnel_e2e.rs:227` always passes `fail2ban_log_path: None`, so there is no existing coverage/pattern for asserting on written log lines; this would be new test surface.