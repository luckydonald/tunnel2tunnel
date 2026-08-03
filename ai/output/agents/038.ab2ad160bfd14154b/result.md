I now have everything needed to answer.

## `tunnel_e2e` test — findings

**File:** `/home/user/git/luckydonald/tunnel2tunnel/crates/t2t/tests/tunnel_e2e.rs` — test fn `two_ssh_connections_tunnel_through_rendezvous` (only test in the file). A related but distinct harness lives in `/home/user/git/luckydonald/tunnel2tunnel/crates/t2t/tests/tarpit_e2e.rs`.

**1. Setup.** DB via `db::connect(DATABASE_URL)` + `sqlx::migrate!("../../migrations")`. Fixtures created via model helpers directly (no raw SQL): `User::create`, `Entity::create` (server_entity_a, server_entity_b, client_entity, one owner), `SshKey::create` per entity, `EntityAccess::create` (grant), `PortSubscription::create` (client opts in to each `port_config`). `port_config` rows aren't created explicitly — they're auto-created by `tcpip_forward` when `ssh -R` registers, then polled via helper `wait_for_port_config`. The SSH server is started in-process via `start_ssh(SshConfig{ssh_port: free_port(), host_key_path: scratch dir, ..}, pool, new_server_slots(), new_active_tunnels())`, spawned on a `tokio::spawn`. **It drives real OpenSSH client binaries via `Command::new("ssh")`** (`spawn_ssh_r`, `spawn_ssh_l` — `-N -R`/`-N -L` subprocesses), deliberately not a Rust client, per the file's doc comment (needed to reproduce OpenSSH's own forwarded-tcpip address matching).

**2. Verification.** Polls port_config/active_tunnels via SQL/model queries and the shared `ActiveTunnels` map (`wait_for_active_tunnel_entry`). Drives real HTTP requests through the local `-L` ports (`fetch_through_tunnel`, retry loop up to a `Duration` timeout, 150ms poll) and asserts response bodies/labels (routing correctness), a POST/echo round-trip, and teardown of `active_tunnels` entries on disconnect. Only rough timing check: `elapsed < 2s` isn't in this file (that's tarpit_e2e); tunnel_e2e itself uses generous timeouts (5–15s) but never asserts an upper bound tight enough to catch a 12s stall on a *second* channel.

**3. Two concurrent channel-opens on the SAME connection?** No. The only concurrency (`tokio::join!` around line 666) is two *different* SSH connections (tunnel A's `-L` and tunnel B's `-L`, each its own subprocess/socket). Every `ssh -L`/`-R` subprocess is its own TCP+SSH connection; there's no scenario here where two `channel_open_direct_tcpip` requests race on one `Session`/`Handle`. This is exactly the gap you suspect.

**4. "Server never registers" helper.** None exists in either test file. `wait_for_port_config` assumes an `-R` side always shows up. To build the offline scenario you'd need to manually insert a `port_config`/`PortSubscription`/`EntityAccess` (or reuse `PortConfig`/`PortSubscription` model helpers directly, bypassing `tcpip_forward`) for a target entity that never calls `tcpip_forward` — i.e. skip `spawn_ssh_r` for that entity/port entirely and construct the `port_config` row yourself via `PortConfig::create`-style helper (check `crates/tunnel2tunnel-core/src/models/port_config.rs` for the exact create signature — not shown in this file).

**5. Second concurrent client / second channel on one handle.** No helper exists for this in `tunnel_e2e.rs`; `spawn_ssh_l`/`spawn_ssh_r` always create brand-new OS processes = brand-new SSH connections, so they can't test single-connection serialization. `tarpit_e2e.rs` is the right pattern to copy instead: it drives russh's **in-process client** directly —
```rust
use russh::client::{connect_stream, Config as ClientConfig, Handle as ClientHandle};
struct TestClient;
impl russh::client::Handler for TestClient {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _: &russh::keys::ssh_key::PublicKey) -> Result<bool, Self::Error> { Ok(true) }
}
async fn connect_client_at(host: &str, port: u16) -> ClientHandle<TestClient> {
    let stream = tcp_connect_from(host, port).await; // TcpSocket bind+connect
    connect_stream(Arc::new(ClientConfig::default()), stream, TestClient).await.expect("client connect")
}
```
After `handle.authenticate_publickey(...)` succeeds, `ClientHandle<H>` exposes:
```rust
pub async fn channel_open_direct_tcpip<A: Into<String>, B: Into<String>>(
    &self, host_to_connect: A, port_to_connect: u32,
    originator_address: B, originator_port: u32,
) -> Result<Channel<Msg>, crate::Error>
```
(`russh-0.61.2/src/client/mod.rs:718`). Because `ClientHandle` is `Clone`-able / shared, you can call this twice from two spawned tasks on the *same* `handle` — that's the missing piece: `tokio::join!(handle.channel_open_direct_tcpip(offline_target_uuid, port, "x", 0), handle.channel_open_direct_tcpip(online_target_uuid, port, "x", 0))`, then assert the online one's `Channel` yields data well under 12s (e.g. `< 1s`) using `tokio::time::timeout` + `channel.wait()`/`ChannelMsg::Data`, proving it isn't serialized behind the offline one's 12s retry loop in `channel_open_direct_tcpip` (`crates/tunnel2tunnel-ssh/src/lib.rs:1214-1243`, `POLL_INTERVAL=250ms`, `MAX_WAIT=12s`).