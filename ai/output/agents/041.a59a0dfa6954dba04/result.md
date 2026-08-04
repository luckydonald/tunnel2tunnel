All three test scenarios are implemented and passing. Full workspace still builds cleanly, and the pre-existing `tarpit_e2e.rs` suite is unaffected.

## What changed

**`crates/t2t/Cargo.toml`** — added `reqwest` (0.13, `json`+`cookies` features only, no TLS backend since all requests are plain `http://127.0.0.1`) and `serde_json` as dev-dependencies, so the test file can drive the real HTTP API instead of re-deriving the route's logic locally. `Cargo.lock` picked up the new transitive deps (`cookie_store`, `publicsuffix`, etc.) automatically.

**`crates/t2t/tests/tunnel_e2e.rs`** — added:
- `spawn_ssh_authenticated_only` — `ssh -N` with no `-R`/`-L` at all; completes auth (flips `entity_status` online) without ever registering a forward.
- `wait_for_entity_online` — polls `ConnectionLog::entity_status`.
- `spawn_web_server` — boots the real `tunnel2tunnel_web::start` axum server on an ephemeral port, sharing the same `ServerSlots`/`ActiveTunnels` the SSH server uses.
- `login` / `get_entity_live_connections` / `find_subscription_row` — real session-cookie login via `POST /api/auth/login`, then `GET /api/entities/{id}/live-connections`, parsed as `serde_json::Value` (kept loose rather than importing the response structs, since those types live in a private module and aren't re-exported).

Test coverage added:
- **`two_ssh_connections_tunnel_through_rendezvous`** (extended) — after both bridges are confirmed live, asserts both subscriber rows report `live: true, remote_status: "green"`, and the service-side row aggregates `live: true, remote_status: null`. After tunnel A's client leg disconnects, asserts that row flips to `live: false, remote_status: "green"` (owner still connected+forwarded, just no bridge for this subscriber).
- **`same_connection_online_target_not_blocked_by_offline_target_timeout`** (extended) — at the end, asserts the fully-offline target's subscription row is `live: false, remote_status: "gray"`, and the online target's is `remote_status: "green"`.
- **`subscriber_sees_orange_ring_when_owner_online_but_port_not_forwarded`** (new) — the core regression case: owner authenticates via `ssh -N` with no forward, test confirms directly against `server_slots` that no entry exists for that entity/port, then asserts via the real HTTP route that the subscriber's row is `live: false, remote_status: "orange"`.

## Test run

```
export DATABASE_URL=postgres://t2t:t2t_secret@localhost:5433/tunnel2tunnel
cargo test -p t2t -- --test-threads=1
```
(Postgres was already provisioned as podman container `t2t-pg`, just stopped — started it; it maps host port 5433, per prior project memory.)

```
running 7 tests   (tarpit_e2e.rs)   ... all ok, 109.55s
running 3 tests   (tunnel_e2e.rs)
test same_connection_online_target_not_blocked_by_offline_target_timeout ... ok
test subscriber_sees_orange_ring_when_owner_online_but_port_not_forwarded ... ok
test two_ssh_connections_tunnel_through_rendezvous ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.64s
```

No changes were made to `crates/tunnel2tunnel-web/src/routes/live_connections.rs` or any frontend file.