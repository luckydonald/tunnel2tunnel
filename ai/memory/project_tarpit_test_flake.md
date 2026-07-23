---
name: project_tarpit_test_flake
description: Known pre-existing flaky e2e test in crates/t2t/tests/tarpit_e2e.rs — repeated_bans_eventually_engage_banner_drip
metadata: 
  node_type: memory
  type: project
  originSessionId: 6b1c7bbf-317f-4cd4-9829-855c18ee5b7b
  modified: 2026-07-23T12:00:18.346Z
---

`repeated_bans_eventually_engage_banner_drip` in `crates/t2t/tests/tarpit_e2e.rs` fails intermittently with "banner-drip must never send the real SSH-2.0 identification line" even on completely unmodified code (confirmed via `git stash` + rerun).

**Why:** all tarpit e2e tests share one long-lived local dev Postgres DB and the same loopback peer_ip (`127.0.0.1`). Once *any* test run (this one or a past session) logs a successful login from `127.0.0.1` (`connection_logs.success_reason = 'correct login'`), `ConnectionLog::peer_ip_has_known_good_history` returns `true` for that peer_ip **forever** (it's a DB history check, not in-memory state that resets between test runs) — which permanently disables banner-drip eligibility for `127.0.0.1` and downgrades that tarpit method to slow-auth. `legit_login_sequence_is_never_tarpitted` (which runs earlier in the same file) is exactly the kind of test that pollutes this.

**How to apply:** if this specific test fails while running the tarpit_e2e suite, don't assume a code change broke it — verify by stashing and rerunning against the same DB, or by wiping/recreating the dev DB. All other tarpit_e2e tests are reliable. Not yet fixed — would need either a fresh DB per test run or a `peer_ip` that varies per test.
