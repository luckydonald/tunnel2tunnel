Repo: /home/user/git/luckydonald/tunnel2tunnel (Rust workspace). Read CLAUDE.md at repo root first.

This is Phase 5 (final phase) of a larger redesign whose plan lives at `/home/user/.confuig/claude/accounts/private/plans/robust-bubbling-ember.md` — **read this file in full**, especially the "Live connections dashboard" section and the four GUI mockups (Dashboard, Admin/Live connections, and the status-dot columns in the entity-detail mockups). Phases 1-4 (schema, core models, SSH enforcement, web routes, and the frontend restructure) are already committed on `mane` — check recent commit log (`git log --oneline -10`) and read `crates/tunnel2tunnel-ssh/src/lib.rs` and `crates/tunnel2tunnel-web/src/routes/entities.rs` as they exist NOW (they've changed since the plan was written) rather than assuming line numbers from the plan file are still accurate.

This task is the **backend portion only** of Phase 5 (in-memory tracking + API routes). A separate follow-up task will build the three frontend surfaces (per-entity status dots, Dashboard, admin page) against the routes you build here — do not touch `frontend/`.

## What to build

1. **In-memory active-tunnel tracking** in `crates/tunnel2tunnel-ssh/src/lib.rs`, alongside the existing `ServerSlots` type (`Arc<Mutex<HashMap<(Uuid, u32), (Handle, String)>>>`, keyed by `(entity_id, proxy_port)` — this already tracks server-side `-R` registrations). Add a second map for client-side bridges (today's `channel_open_direct_tcpip` already builds `self.bridges: HashMap<ChannelId, (Handle, ChannelId)>` per-connection but nothing aggregates that across connections or exposes it):
   ```rust
   pub type ActiveTunnels = Arc<Mutex<HashMap<Uuid, ActiveTunnelInfo>>>; // keyed by a fresh bridge id (Uuid::now_v7())
   pub struct ActiveTunnelInfo {
       pub client_entity_id: Uuid,
       pub client_user_id: Uuid,
       pub target_entity_id: Uuid,
       pub port_config_id: Uuid,       // whichever port_configs row this bridge is using
       pub proxy_port: u32,
       pub peer_ip: String,
       pub since: time::OffsetDateTime,
   }
   ```
   Insert an entry when `channel_open_direct_tcpip`'s bridging succeeds (after the 4-step check from the prior phase passes and the bridge is actually established) — find the current implementation's success path and read where `self.bridges.insert(...)` happens now, insert alongside it. Track which `active_tunnels` key belongs to which `ChannelId` (a small parallel `HashMap<ChannelId, Uuid>` field on the handler struct, or fold the bridge id into the existing bridges map's value tuple — your call) so it can be removed on `channel_close`/`channel_eof` and in the `Drop for T2tHandler` cleanup (mirror the existing pattern there that already retains/cleans `server_slots` on disconnect — find and match it).
   Wire `ActiveTunnels` into the SSH server's top-level state/startup and into each new `T2tHandler` the same way `ServerSlots` already is (find where `server_slots` gets threaded through `Arc::new(Mutex::new(HashMap::new()))` at startup and cloned per-connection, and add the equivalent for `active_tunnels`).

2. **New web routes**, `crates/tunnel2tunnel-web/src/routes/live_connections.rs` (new file, register it in `crates/tunnel2tunnel-web/src/lib.rs`/router alongside the other route modules — follow the existing module registration pattern, e.g. how `tarpit.rs`'s routes are wired in):
   - `GET /api/entities/{id}/live-connections` — owner-only (reuse the existing `require_owner` pattern from `entities.rs`). Returns rows relevant to *this* entity only:
     - If it owns any `port_configs`: for each, whether it's live (an entry exists in `server_slots` for `(entity_id, proxy_port)`) plus the list of currently-bridged subscribers hitting it (look up `active_tunnels` entries where `target_entity_id == this entity` and `proxy_port` matches that port_config — join in the subscriber's entity name/user for display).
     - If it owns any `port_subscriptions`: for each, its own status — green if there's an `active_tunnels` entry for `(client_entity_id == this entity, port_config_id == that subscription's port_config)`, gray if not currently live, orange specifically if the subscription is `enabled` but the *owner's* port_config has no `server_slots` entry at all (server-side genuinely down) — distinguish this from the plain "gray/not connected yet" case per the plan's status-dot semantics.
   - `GET /api/admin/live-connections` — admin-only (find and reuse the existing admin-gating pattern, e.g. how `tarpit.rs`'s admin routes or `routes/admin.rs` check for admin role). Returns one row per leg, flattened across all entities: every `port_configs` row with its live/not-live status (a "server-role" row), and every `port_subscriptions` row with its live/gray/orange status (a "client-role" row) — joined with `entities`/`users` for account/entity-name context.
   - Row shape (shared struct, roughly): `{ status: "green"|"gray"|"orange", account: { user_id, username }, entity: { id, name }, role: "server"|"client", service_name: string, port: i32, peer_ip: Option<String>, connected_since: Option<OffsetDateTime> }`. Reuse whatever existing response-building helpers `entities.rs` already has for entity/user lookups rather than re-querying from scratch if there's a convenient shared helper — check first.
   - This requires threading `ActiveTunnels`/`ServerSlots` from the `tunnel2tunnel-ssh` crate into the web crate's `AppState` — find where both the SSH server and the web `AppState` are constructed together (likely `crates/t2t/src/main.rs`) and add the two `Arc<Mutex<...>>` handles to `AppState`, passing the same instances used by the SSH server so they reflect real-time state.

3. **Status semantics reminder** (from the plan, don't reinvent): green = live entry exists for that `(entity, port)`; gray = configured/subscribed but no live entry; orange = subscriber-side only, when the subscription is enabled but the owner's port has no live `server_slots` entry at all (i.e., distinctly "the other side is down," not just "haven't connected yet" — if you can't cleanly distinguish "haven't tried yet" from "tried and pending" given the current retry-in-`channel_open_direct_tcpip` design, treat any enabled-subscription-with-no-active-bridge-and-no-live-server-slot as orange; that's the practically meaningful signal here).

## Verification

- `cargo build --workspace` clean; `cargo test --workspace` (there's a real e2e SSH test at `crates/t2t/tests/tunnel_e2e.rs` exercising the 4-step enforcement from the prior phase — consider extending it, or adding a small new test, to confirm an active tunnel actually shows up via the new `GET .../live-connections` route while a real bridged connection is open, and disappears after it closes; check how the existing e2e test drives real `ssh` client processes and mirror that pattern if you add to it).
- Do not touch `frontend/`.

## Committing

`commit-with-lplp-style` skill is active — write the message to `ai/git/pending-commit.md` first, format `[backend] live connections: ai: Run: <summary>.`, stage only files you changed by explicit path. One or two commits is fine for this scope (e.g. tracking+SSH-layer wiring, then routes/AppState plumbing).

Report back: exact new route paths and response JSON shape (field names/types) so a follow-up frontend task can consume them precisely, any deviations from the plan and why, and final `cargo build`/`cargo test` status.