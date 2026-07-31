Everything builds cleanly across all three commits. The backend implementation is complete and committed.

## Summary

I implemented the backend portion (sequencing steps 1–3) of the port-configs + subscriptions redesign, replacing `entity_ports`/`entity_port_discovery_rules` with `port_configs`/`port_subscriptions`, and dropping `entities.type` in favor of computed `is_server`/`is_client` booleans.

**Migrations**: `/home/user/git/luckydonald/tunnel2tunnel/migrations/014_port_configs_subscriptions.sql` (rename+clean `entity_ports`→`port_configs`, drop `entity_port_discovery_rules`, create `port_subscriptions`, extend `entity_access` with `port_config_id`) and `015_drop_entity_type.sql`.

**Core models** (`crates/tunnel2tunnel-core/src/models/`): `port_config.rs` (renamed from `entity_port.rs`), new `port_subscription.rs` (replaces `entity_port_discovery_rule.rs`, includes the `list_subscribable_for_entity` browse query), `entity_access.rs::check_access` extended with `port_config_id` + a same-account short-circuit, `entity.rs` stripped of `entity_type`. New `crates/tunnel2tunnel-core/src/port_names.rs` for auto-naming.

**SSH layer** (`crates/tunnel2tunnel-ssh/src/lib.rs`): `tcpip_forward` auto-creates a named `port_configs` row on first `-R`; `channel_open_direct_tcpip` implements the exact 4-step check from the plan (port_config match → subscription check → live access re-check → bounded 250ms/12s retry on server liveness), each rejection with a distinct log line.

**Web routes** (`crates/tunnel2tunnel-web/src/routes/entities.rs`, `access.rs`, `lib.rs`): ports CRUD retargeted at `PortConfig`; new `GET .../subscribable-services` and `POST/PUT/DELETE .../subscriptions` routes; `EntityResponse` gains computed `is_server`/`is_client` (batched, no N+1) and `?role=server|client` list filtering; `CreateAccessBody` gains validated `port_config_id`.

One deviation worth flagging: I added a `PortConfig::entities_have_any`/`PortSubscription::entities_have_any` batching helper (HashSet-based) rather than mirroring `ConnectionLog::entity_statuses`'s exact HashMap shape — simpler since these are pure booleans, not `(bool, Option<timestamp>)` pairs.

I also had to fix three pre-existing test files that referenced the old signatures (`crates/t2t/tests/tarpit_e2e.rs`, `tunnel_e2e.rs`, `crates/tunnel2tunnel-core/tests/tarpit_models.rs`) — not in the original file list but necessary for `cargo test --workspace` to compile. `tunnel_e2e.rs` now waits for the auto-created `port_configs` row and registers a `port_subscriptions` row before the `-L` leg connects, since a bare `entity_access` grant alone no longer suffices.

**Verification**: `cargo build --workspace` is clean. `cargo test -p t2t --test tunnel_e2e` passes (validates the real end-to-end four-step SSH enforcement with real `ssh` binaries). `tarpit_e2e` (7 tests) passes. Four `tarpit_models` test failures are pre-existing and unrelated — confirmed by stashing all my changes and re-running against the same (already-migrated) dev DB, where they fail identically; they're a dynamic-SQL bug in `ConnectionLog::search` and shared-DB test pollution, untouched by this work.

Full new/changed API shapes for the frontend follow-up are written out in the third commit message (`eabeb1d`) for exact reference. `frontend/` was not touched.

result: Backend port-configs/subscriptions redesign (schema, core models, SSH enforcement, web routes) implemented across 3 commits (`ab2997c`, `9077ef2`, `eabeb1d`) on `mane`; `cargo build --workspace` clean and the real SSH e2e test passes against the new four-step enforcement.