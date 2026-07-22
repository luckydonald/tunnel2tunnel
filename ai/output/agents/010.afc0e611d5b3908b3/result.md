# Research Report: t2t Integration Test Feasibility

## 1. Existing test code

None beyond unit tests in `tunnel2tunnel-core`. No `tests/` directory exists in any crate.

- `crates/tunnel2tunnel-core/src/pubkey.rs:39,48,57,67` — 4 `#[test]` fns (`mod tests`) covering `parse_authorized_keys_line` / `compute_fingerprint`.
- `crates/tunnel2tunnel-core/src/ip_whitelist.rs:80-118` — 7 `#[test]` fns for the whitelist evaluator.
- No `#[tokio::test]`, no `#[sqlx::test]`, anywhere.
- `tunnel2tunnel-web`, `tunnel2tunnel-ssh`, `t2t`, `t2t-feasibility` have zero test code.

This will be the first integration test in the workspace — there's no existing harness/pattern to follow.

## 2. Dependencies

- **No `[dev-dependencies]` section exists in any `Cargo.toml`** (workspace root or any crate).
- **`tempfile`** — present in `Cargo.lock:3684` only as a *transitive* dep (pulled in via `native-tls`, itself pulled by `reqwest`). Not a direct dependency anywhere; you saw it compile because of the sentry→reqwest→native-tls chain, not because the project uses it for tests.
- **`reqwest`** — present in `Cargo.lock:2628`, also transitive: it's `sentry`'s HTTP transport (`sentry = { ..., features = [...] }` in root `Cargo.toml`, and `sentry`'s `transport` feature pulls reqwest w/ native-tls per the note in `CLAUDE.md:210-211` — do NOT switch it to rustls, that's an explicit documented gotcha). Not used directly by any of your own code, and not declared as a direct dependency in any crate's `[dependencies]`.
- **sqlx features** — workspace `Cargo.toml:9` declares `sqlx = { version = "0.8", features = ["postgres", "uuid", "time", "runtime-tokio"] }` with default-features left on. sqlx 0.8.6's `default = ["any", "macros", "migrate", "json"]` (`~/.cargo/registry/.../sqlx-0.8.6/Cargo.toml:397-402`), so **`migrate` is already enabled** (proven in use — see #6). `test-util` is not enabled/needed since nothing uses `#[sqlx::test]` or compile-time-checked `query!`/`query_as!` macros anywhere (grep confirmed zero hits) — all queries use runtime `sqlx::query_as::<_, T>(...)`, so **no `DATABASE_URL` or `.sqlx` cache is needed just to compile the workspace**.
- **No testcontainers dependency**, no existing pattern for spinning up a test DB. `MANUAL_TESTING.md:9-34` and `CLAUDE.md:38-41` both document a manual Podman-based Postgres 18 setup (`podman run ... docker.io/postgres:18-alpine`) as the accepted way to get a DB locally — this is the pattern you'd reuse/automate, there's no testcontainers-rs crate anywhere in the lockfile.

## 3. Programmatic startup / ephemeral ports / global state

- `crates/tunnel2tunnel-web/src/lib.rs:150-153`:
  ```rust
  let addr = format!("0.0.0.0:{}", config.http_port);
  let listener = tokio::net::TcpListener::bind(&addr).await?;
  axum::serve(listener, app).await?;
  ```
  Binding port `0` works fine (`TcpListener::bind` supports it), **but `start()` never returns/exposes the bound `SocketAddr`** — it consumes the listener straight into `axum::serve`. To learn the OS-assigned port you'd have to either (a) bind your own `TcpListener` first via `std::net::TcpListener::bind("127.0.0.1:0")`, read `.local_addr().port()`, drop it, and pass that number in as `http_port` (small TOCTOU race, acceptable in practice), or (b) patch `start()` to return the addr (out of scope, read-only research).
- `crates/tunnel2tunnel-ssh/src/lib.rs:80-105`, specifically `server.run_on_address(russh_config, ("0.0.0.0", config.ssh_port)).await?`. Traced into the vendored `russh-0.61.2` source: `run_on_address` (`~/.cargo/registry/.../russh-0.61.2/src/server/mod.rs:970-983`) is just `let socket = TcpListener::bind(addrs).await?; self.run_on_socket(...)`. Port 0 works at the socket level, but same problem: `tunnel2tunnel_ssh::start()` doesn't return the bound port, and `T2tServer`/`T2tHandler` are private (not `pub`), so you can't drop down to `run_on_socket` yourself from outside the crate to recover the real port. **Practical workaround for both servers: pre-allocate free ports the same way** (bind-then-drop trick) and pass fixed numbers into `WebConfig`/`SshConfig`.
- **No global/static state anywhere** — grepped all crates for `static `, `lazy_static`, `once_cell`, `OnceCell`, `OnceLock`: zero hits outside an unrelated `&'static str` field type and one log line. `AppState`, `SshConfig`/`WebConfig`, `T2tServer`'s `ServerSlots`/`SessionRegistry` are all instance-local `Arc<Mutex<...>>` constructed fresh inside `start()`. Running two independent `start_http`/`start_ssh` pairs (different ports, different/same pool) in the same test binary, sequentially or concurrently, should be safe — nothing prevents it.
- `tracing_subscriber::fmt().init()` and Sentry init (`crates/t2t/src/main.rs:13-17`) only happen in the `t2t` binary's `main()`, not inside the library `start()` functions — so calling `tunnel2tunnel_web::start` / `tunnel2tunnel_ssh::start` directly from a test doesn't touch global logging/Sentry state at all. (If you do want tracing output in the test, use `tracing_subscriber::fmt().try_init()` and ignore the `Err` if called more than once across tests.)
- Both `start()` functions are `async fn` returning `anyhow::Result<()>` that **run forever** (`axum::serve(...).await` / the russh server loop) — in a test you must `tokio::spawn` each and not await them directly, then interact via the ports, then simply drop the `JoinHandle`s (or abort them) at the end of the test.

## 4. Bypassing the HTTP API for setup

Yes — fully feasible and much simpler than session-cookie HTTP calls. Everything needed is a plain public associated fn on a public struct taking `&PgPool`:

- `crates/tunnel2tunnel-core/src/models/user.rs:42-68` — `User::create(pool, username, email, password, is_admin, description)`. (`bootstrap_admin` in `crates/tunnel2tunnel-web/src/bootstrap.rs:4-12` is just a thin idempotent wrapper around this — you can call `User::create` directly instead, or just reuse `bootstrap_admin`, though you don't strictly need a user named "admin"/logged-in-via-cookie for this test at all since you're bypassing HTTP.)
- `crates/tunnel2tunnel-core/src/models/entity.rs:71-97` — `Entity::create(pool, user_id, entity_type, name, description, ip_whitelist, valid_until)`. `entity_type` is a free-form string column (`"server"`/`"client"` per convention, not a DB enum — no extra validation to satisfy).
- `crates/tunnel2tunnel-core/src/models/ssh_key.rs:61-89` — `SshKey::create(pool, entity_id, algorithm, key_data, comment, name, valid_until)`. **Important**: `key_data` must be *just the base64 blob* (not the full "algo base64 comment" line) — `SshKey::create` internally calls `crate::pubkey::compute_fingerprint(key_data)` (`ssh_key.rs:70`) which base64-decodes it directly (`crates/tunnel2tunnel-core/src/pubkey.rs:26-33`). This exactly matches what `MANUAL_TESTING.md:117-118` says about the HTTP API too.
- `crates/tunnel2tunnel-core/src/models/entity_access.rs:52-76` — `EntityAccess::create(pool, owner_entity_id, subject_type, subject_entity_id, subject_user_id, hostname)`. For your scenario: `owner_entity_id = server_entity.id`, `subject_type = "entity"`, `subject_entity_id = Some(client_entity.id)`.

All of these are re-exported publicly: `crates/tunnel2tunnel-core/src/lib.rs:5` (`pub mod models`) and `models/mod.rs:1-9` (`pub mod entity; pub mod ssh_key; pub mod entity_access; pub mod user;`). `CoreError` is also `pub use`d (`lib.rs:9`). So a test can add `tunnel2tunnel-core` as a dependency and call these directly against a pool from `tunnel2tunnel_core::db::connect(...)` (`crates/tunnel2tunnel-core/src/db.rs:4-10`), skipping HTTP/cookies entirely for setup, then only use real `ssh` subprocesses for the actual tunnel traffic.

Login route (`crates/tunnel2tunnel-web/src/routes/auth.rs:35-58`) is trivial if you ever do want to exercise the HTTP layer too — it's just `User::find_by_username` + `verify_password` + `session.insert("user_id", ...)` — but not needed for this test's setup phase.

## 5. Generating SSH keypairs in Rust (no `ssh-keygen`)

Fully reusable — `load_or_generate_host_key` in `crates/tunnel2tunnel-ssh/src/lib.rs:862-911` is the exact template. Relevant imports (`lib.rs:8-11`):

```rust
use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::keys::{Algorithm, PrivateKey};
use russh::keys::ssh_key::LineEnding;
```

Key generation call (`lib.rs:886-887`):
```rust
let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)?;
```

For your test you don't need `write_openssh_file`/host-key persistence logic (that's disk caching, irrelevant for ephemeral test keys) — you need to get an OpenSSH-authorized_keys-style public key line to split into `(algorithm, key_data)` for `SshKey::create`, and the private key in a form the `ssh` CLI subprocess can use via `-i`. Traced through the vendored crate (russh 0.61.2 re-exports `ssh_key` wholesale — `~/.cargo/registry/.../russh-0.61.2/src/keys/mod.rs:78`: `pub use ssh_key::{self, Algorithm, Certificate, EcdsaCurve, HashAlg, PrivateKey, PublicKey};`):

- `key.public_key()` → `ssh_key::PublicKey`, whose `.to_openssh()` (`~/.cargo/registry/.../ssh-key-0.7.0-rc.10/src/public.rs:178`) returns the full `"ssh-ed25519 AAAA... [comment]"` line — feed that straight into the existing `tunnel2tunnel_core::pubkey::parse_authorized_keys_line` (`crates/tunnel2tunnel-core/src/pubkey.rs:7-22`) to get `(algorithm, key_data_b64, comment)` for `SshKey::create`.
- For the private key that the `ssh` subprocess needs on disk: `key.write_openssh_file(path, LineEnding::LF)` (used at `lib.rs:900-902`), writing to files under a temp dir with `0600` perms (set that explicitly, or `ssh` will refuse the key on some platforms/configs) — same call shape as the host key.

So the minimal shape per test keypair is:
```rust
use getrandom::{rand_core::UnwrapErr, SysRng};
use russh::keys::{Algorithm, PrivateKey, ssh_key::LineEnding};
use tunnel2tunnel_core::pubkey::parse_authorized_keys_line;

let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)?;
key.write_openssh_file(&priv_path, LineEnding::LF)?;   // for `ssh -i priv_path`
let pub_line = key.public_key().to_openssh()?;          // "ssh-ed25519 AAAA... "
let (algorithm, key_data, _comment) = parse_authorized_keys_line(&pub_line)?;
// SshKey::create(pool, entity_id, &algorithm, &key_data, Some("test-server"), None, None).await?;
```
No shelling out to `ssh-keygen` needed; `russh` (already a workspace dep) is sufficient end-to-end.

## 6. Migration runner reuse

`crates/t2t/src/main.rs:54-57`:
```rust
sqlx::migrate!("../../migrations")
    .run(&pool)
    .await
    .context("failed to run database migrations")?;
```
This is the only `sqlx::migrate!` call in the workspace. The path is resolved relative to `CARGO_MANIFEST_DIR` at compile time, not the invoking file's location — so it works identically from any `tests/*.rs` file placed inside `crates/t2t/`, `crates/tunnel2tunnel-web/`, or `crates/tunnel2tunnel-ssh/` (all are one level under `crates/`, so `"../../migrations"` resolves to `<repo_root>/migrations` from any of them). Migrations directory: `migrations/000_helpers.sql` through `007_entity_port_host.sql` (`ls` above) — 8 files including `000_helpers.sql` which defines the shared `set_timestamps()` trigger (`CLAUDE.md:99`).

One extra thing your test setup must replicate manually (since you're calling the library `start()` fns directly rather than going through `t2t`'s `main()`): `tunnel2tunnel_web::start()` (`lib.rs:44-45`) only runs `PostgresStore::new(pool).migrate()` for the **session table**; it does *not* run the app schema migrations or bootstrap the admin user. Your test needs to explicitly call `sqlx::migrate!("../../migrations").run(&pool).await?` (and, if you still want a User row for `Entity::create`'s `user_id`, either `User::create` directly or `bootstrap_admin`) before calling `start_http`/`start_ssh`, mirroring `main.rs:51-65`.

## 7. CI

**No CI runs `cargo test` today, and no CI provisions Postgres.** All three workflow files under `.github/workflows/` are Claude/Codex automation agents, not test pipelines:
- `.github/workflows/claude.yml`
- `.github/workflows/claude-issue-agent.yml`
- `.github/workflows/codex-issue-agent.yml`

None contain a `services:` block, `cargo test`, or any Postgres provisioning (grepped for all three, zero hits). So a new DB-dependent integration test **will not run in CI as-is** — it would only ever execute locally (or wherever a dev/agent manually starts Postgres per `MANUAL_TESTING.md`/`CLAUDE.md:38-41`) unless you also add a new workflow (or extend an existing one) with a `postgres:18` service container and a `cargo test` step. Until that exists, you should gate the test (e.g. `#[ignore]`, or skip when `DATABASE_URL` env var / a running Postgres isn't detected) so a plain `cargo test` in this repo doesn't fail for contributors without a local Postgres 18 instance running.

## Summary of blockers/gotchas

1. **No test scaffolding exists at all** — this will be a from-scratch `tests/` integration test (recommend placing it in `crates/t2t/tests/` since that's the crate that wires all three pieces together, and it keeps the `sqlx::migrate!("../../migrations")` path identical to `main.rs`).
2. **Need `tunnel2tunnel-core`, `tunnel2tunnel-web`, `tunnel2tunnel-ssh` as `[dev-dependencies]`** of wherever the test lives (none currently declared — `t2t/Cargo.toml` already has them as regular deps if you put the test in that crate, so nothing to add there).
3. **`tempfile` and `reqwest` are not real project dependencies** — they're transitive (via sentry/native-tls). If you want `tempfile` for scratch dirs/host-key paths in the test, add it explicitly as a dev-dependency (it'll already be in `Cargo.lock` so no new version resolution surprises).
4. **Neither `start()` function returns/exposes the bound port** — plan to pre-bind ephemeral ports yourself (`std::net::TcpListener::bind("127.0.0.1:0")`, read the port, drop, reuse the number) rather than relying on port 0 pass-through.
5. **`tunnel2tunnel_web::start()` doesn't run app migrations or bootstrap a user** — the test must do `sqlx::migrate!` + `User::create`/`bootstrap_admin` itself before starting the servers.
6. **No global state** — safe to spin up both servers in-process; just remember `start()` never returns (must `tokio::spawn` and not await it).
7. **No CI Postgres today** — gate/ignore the test or accept it's local-only until a workflow with a `postgres:18` service is added.
8. **Key data format gotcha** carried over from the manual procedure: `SshKey::create`'s `key_data` param is the bare base64 blob, not the full `"ssh-ed25519 <b64> comment"` line — use `pubkey::parse_authorized_keys_line` on the `to_openssh()` output to split it correctly.