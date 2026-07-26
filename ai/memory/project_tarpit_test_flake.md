---
name: project_tarpit_test_flake
description: "RESOLVED — repeated_bans_eventually_engage_banner_drip flake in crates/t2t/tests/tarpit_e2e.rs, fixed 2026-07-26"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4d8f0732-ba86-40c0-a1c7-3306e46d69ff
  modified: 2026-07-26T17:05:10.847Z
---

Previously flaky test `repeated_bans_eventually_engage_banner_drip` (`crates/t2t/tests/tarpit_e2e.rs`) is now fixed (commit `a1950ab`, 2026-07-26).

**What the earlier theory got wrong:** an older version of this memory blamed shared-DB `peer_ip = '127.0.0.1'` history pollution (`ConnectionLog::peer_ip_has_known_good_history`) and suggested varying `peer_ip` per test as the fix. A first attempt to do exactly that — dialing a different `127.x.x.x` loopback address as the *destination* — did not fix it, because **the destination address does not determine which local/source address the OS kernel picks for an outgoing loopback connection**; it defaults to `127.0.0.1` regardless of which loopback address you dial. So `peer_ip` as observed server-side never actually varied.

**Real fix:** build the client socket by hand with `tokio::net::TcpSocket`, explicitly `.bind()` it to the desired source address, then `.connect()` — see `tcp_connect_from`/`connect_client_at` in `tarpit_e2e.rs`, and `random_loopback_ip()` for generating a fresh never-before-used `127.0.0.0/8` address per test run.

**How to apply:** if a similar "isolate this test by using a different peer/client IP" need comes up again in this repo (or any Rust+tokio test suite dialing `127.0.0.1`), remember: vary the *bound source address*, not the destination — on Linux, all of `127.0.0.0/8` routes to loopback, so any `127.x.x.x` triple works as a distinct identity once actually bound as the socket's local address.
