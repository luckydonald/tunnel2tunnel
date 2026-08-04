Repo: /home/user/git/luckydonald/tunnel2tunnel (Rust workspace + Vue frontend). You are implementing test coverage for a just-finished backend change; do NOT touch frontend or the already-modified backend logic file (only add tests, and only add a new SSH-connect-without-forward test helper if genuinely needed — do not change existing route logic).

## What changed (already implemented and unit-tested, do not modify)

`crates/tunnel2tunnel-web/src/routes/live_connections.rs` used to expose a single `status: "green"|"gray"|"orange"` field. It now exposes two independent fields per row:

- `live: bool` — dot. For subscription rows: true iff this subscriber currently has an active bridge (own channel open). For service (owner) rows: true iff at least one current subscriber has a live bridge (aggregated). For admin `LiveConnectionRow` server-role rows: same aggregation as service rows.
- `remote_status: Option<RemoteStatus>` — ring; only present (non-`None`) for subscription-side rows (service/server rows always have `remote_status: None`). `RemoteStatus` is a serde enum serializing as lowercase `"gray"|"orange"|"green"`:
  - `Gray` — owner entity not SSH-connected at all (or subscription disabled).
  - `Orange` — owner entity SSH-connected, but hasn't forwarded this specific port yet (`server_slots` has no entry for `(owner_entity_id, proxy_port)`). **This is the previously-reported bug's exact scenario**: a client authenticates over SSH (shows up as `entity.online = true` via `ConnectionLog::entity_status`) but hasn't yet issued `tcpip_forward` for the port in question — before this change, the port dot just stayed gray with no signal that the client is actually there.
  - `Green` — owner entity SSH-connected AND that port is forwarded (present in `server_slots`).

Helper functions (already implemented, already unit-tested, in `live_connections.rs`):
```rust
fn subscription_live(enabled: bool, has_active_bridge: bool) -> bool
fn subscription_remote_status(enabled: bool, owner_online: bool, owner_port_live: bool) -> RemoteStatus
```
Routes affected: `list_entity_live_connections` (returns `EntityLiveConnectionsResponse { services: Vec<ServiceLiveStatus>, subscriptions: Vec<SubscriptionLiveStatus> }`) and `list_admin_live_connections` (returns `Vec<LiveConnectionRow>`). Read the current file to see exact field names/shapes before writing test assertions — don't guess.

`entity.online` is computed from `tunnel2tunnel_core::models::connection_log::ConnectionLog::entity_status`/`entity_statuses` — true iff there's a `connection_logs` row with `success = true AND ended_at IS NULL` for that entity. This gets set the moment SSH auth succeeds, independent of whether any port forward has been requested yet.

## Your task

Add test coverage in `crates/t2t/tests/tunnel_e2e.rs` (1143 lines, real-process SSH e2e test using actual `ssh` binary + a real Postgres DB — read the whole file first to understand its helpers and patterns before editing). Existing helpers you should reuse: `wait_for_port_config` (~line 296), `wait_for_active_tunnel_entry` (~line 321). Existing scenario tests: `two_ssh_connections_tunnel_through_rendezvous` (~line 352) and `same_connection_online_target_not_blocked_by_offline_target_timeout` (~line 837) — read both fully as templates for spawning SSH client subprocesses (`ssh -R` for a server/owner entity, `ssh -L` for a subscriber/client entity), waiting for tunnel state, and cleanup via `ChildGuard`.

Add coverage (extend existing tests and/or add new ones — whichever fits the file's existing structure and conventions best) for these scenarios, asserting on the actual HTTP JSON response from `GET /api/entities/{id}/live-connections` (or by calling the route function directly against test state, whichever this test file's existing pattern already does — check how it currently talks to the running server, e.g. via reqwest or via direct axum state calls):

1. **Owner authenticated over SSH but hasn't forwarded the port yet** → subscriber's row should show `live: false, remote_status: "orange"`. This is the core regression case for the bug fix; it did not exist as a test before. You will likely need a new small helper that spawns/authenticates an `ssh` client connection *without* issuing the `-R` forward request for the port under test (e.g. connect and auth only, or authenticate then hold the connection open without requesting the forward) — build this by adapting the existing SSH-spawning code in this file (look at how `-R` is invoked and how to omit just that flag/request while keeping the auth phase). If constructing "authenticated but no forward" via the real `ssh` CLI is impractical (e.g. `ssh -N` with no `-R` at all might work — it still completes auth and holds the connection open without requesting any forward), prefer that over building a custom low-level SSH client. Confirm this actually produces `entity_status` online=true with no `server_slots` entry, by checking against the same DB/state the existing tests already assert against.
2. **Fully bridged (both sides forwarding + a subscriber's channel actually open)** → assert `live: true` and (if you also check owner online) `remote_status: "green"`. Extend `two_ssh_connections_tunnel_through_rendezvous` for this rather than adding a new test, if a natural insertion point exists after the existing bridge-liveness assertions.
3. **Owner fully offline (never connected, or disconnected/torn down)** → assert `live: false, remote_status: "gray"`. The existing offline-target test or the teardown/drop pattern already in the file (~line 737-765) should cover most of this; add the JSON-field assertions there if not already present.

Also add focused unit-style coverage is NOT needed here (already done in `live_connections.rs`'s own `#[cfg(test)] mod tests` — don't duplicate).

## Constraints
- Do not modify `crates/tunnel2tunnel-web/src/routes/live_connections.rs` — it's finished and unit-tested.
- Do not touch any frontend files.
- Follow this file's existing conventions exactly (naming, comment style, `ChildGuard` usage, polling helpers, timeouts) rather than inventing new patterns.
- After changes, run `cargo test -p t2t --test tunnel_e2e -- --test-threads=1` (requires a reachable Postgres — check if `DATABASE_URL` env var is set in the environment or if there's a local Postgres already running per the project's `podman run ... postgres:18-alpine` setup documented in CLAUDE.md at the repo root; start one via podman if none is running and none is documented as already up, using the exact command from CLAUDE.md's "Running locally" section). Report full pass/fail output.
- If a genuinely new scenario test is warranted rather than extending existing ones, name it clearly (e.g. `subscriber_sees_orange_ring_when_owner_online_but_port_not_forwarded`) following this file's existing naming style.

Report back: what you changed (file diff summary), the new/extended test names, and the full test run output (pass/fail, and any errors with their exact messages if something didn't pass — do not paper over failures).