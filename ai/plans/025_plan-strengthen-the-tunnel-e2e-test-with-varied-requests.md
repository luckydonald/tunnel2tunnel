# Plan: Strengthen the tunnel e2e test with varied requests

## Context

`crates/t2t/tests/tunnel_e2e.rs` (`two_ssh_connections_tunnel_through_rendezvous`)
already satisfies most of what was asked: it uses the real OS `ssh` binary for
both legs (not a Rust SSH client), and it already sets up **two separate
entities with two separate generated Ed25519 keypairs** — a "server" entity
doing `-R` and a "client" entity doing `-L` — matching the "two different
client/server profiles with separate ssh keys" requirement.

What it does *not* do yet: its only traffic-flow verification is
`fetch_through_tunnel`, called twice, issuing the exact same bare
`GET / HTTP/1.1` request both times and checking for the same fixed
`FIXTURE_BODY` string. That doesn't prove request/response framing survives
distinct requests through the tunnel (different paths, different methods,
different bodies, differently-sized payloads) — it only proves the pipe stays
open across two identical round-trips. The ask is to send *multiple different
requests* and assert on each independently, which is a meaningfully stronger
regression guard (e.g. it would catch a bug where the tunnel truncates or
corrupts request/response bytes for anything other than the first fixed
payload).

Goal: extend the existing test in place (not add a new file — it already has
all the required scaffolding) so it (a) drives several distinct HTTP requests
through the same established tunnel and asserts each gets back the correct,
distinct response, and (b) stands up a **second server entity** (its own
keypair, own fixture service, own proxy port) so the test also proves
requests get routed to the *correct* backend — i.e. the client's `-L` for
server A's UUID never accidentally lands on server B's fixture service, and
vice versa. Today's test only ever has one server entity, so it can't catch
a routing bug where t2t picks the wrong `server_slots`/`entity_id` entry.

## Approach

All changes are in `crates/t2t/tests/tunnel_e2e.rs`.

0. **Add a second server entity/profile to check routing.** Alongside the
   existing `server_entity` + `server_key_path`, create a `server_entity_b`
   with its own generated keypair (`server_key_b_path`), its own
   `spawn_fixture_service()` instance (so it can return a body that's
   trivially distinguishable from server A's, e.g. `"hello from SERVER A"` /
   `"hello from SERVER B"`), and grant the client entity access to it the
   same way as server A (`EntityAccess::create`). Register a *second*,
   distinct `PROXY_PORT_B` constant and spawn a second `ssh -N -R` leg for
   it. Then run a second `ssh -N -L` leg from the same client entity/key
   pointed at `server_entity_b`'s UUID on `PROXY_PORT_B`, with its own local
   port. Assert:
   - fetching through the server-A tunnel returns server A's body and never
     server B's,
   - fetching through the server-B tunnel returns server B's body and never
     server A's,
   - both tunnels stay independently usable at the same time (interleave a
     couple of fetches across both local ports).
   This exercises the actual routing key (`(entity_id, proxy_port)` in
   `server_slots`, plus the `port_config`/`port_subscription` lookup) rather
   than just the pipe-stays-open behavior the single-server test already
   covers. Reuses the same `generate_test_keypair`/`fetch_through_tunnel`/
   `ChildGuard`/`log_child_stderr` helpers already in the file — no new
   scaffolding needed beyond the second entity/key/service/proxy-port set.

1. **Generalize the fixture service** (`spawn_fixture_service`, currently
   ignores the request and always replies with `FIXTURE_BODY`). Change it to
   take a `label: &str` parameter (`"A"` / `"B"`) and become a minimal
   request-aware responder:
   - Parse just the request line (method + path) out of the raw bytes it
     already reads.
   - Table of routes, each returning a distinct fixed body prefixed with the
     label (so server A's and server B's responses are never confusable with
     each other, on top of being distinct per-route):
     - `GET /` → `"{label} hello from the tunneled service"` (keeps the
       first-contact assertion close to today's behavior)
     - `GET /alpha` → `"{label} alpha response body"`
     - `GET /beta` → `"{label} beta response body"` (different length than
       alpha, to catch any hardcoded `Content-Length` bugs)
     - `POST /echo` with a request body → echoes the body back verbatim,
       unprefixed (this exercises client→server bytes flowing through the
       tunnel, not just server→client, since the existing test only ever
       sent a bare GET with no body)
   - Anything else → `404`, to make an unrouted request an obvious failure
     rather than a silent success.
   - `spawn_fixture_service("A")` backs server A, `spawn_fixture_service("B")`
     backs server B (see step 0).

2. **Generalize `fetch_through_tunnel`** into something like
   `fetch_through_tunnel(port, method, path, body, timeout)` that builds the
   appropriate request bytes and returns the raw response, keeping the same
   "retry until it succeeds or timeout" structure (still needed for the first
   request, since it races the `-R`/`-L` handshake). Requests after the first
   successful one don't need the retry loop (the tunnel is already up) but
   reusing the same helper is simpler and harmless.

3. **Replace the two identical `fetch_through_tunnel` calls** with a
   sequence exercising the new routes over *both* established tunnels
   (server A's `local_port_a`, server B's `local_port_b` from step 0):
   - `GET /` on tunnel A → assert body starts with `"A "` (first-contact,
     same role as today's existing assertion)
   - `GET /` on tunnel B → assert body starts with `"B "`, and assert it's
     not equal to tunnel A's `/` response — this is the actual routing
     check: proves the client's two `-L` legs each reach their own intended
     server and not each other's
   - `GET /alpha` and `GET /beta` on tunnel A → assert the `"A "`-prefixed
     bodies, and that they differ from each other
   - `POST /echo` on tunnel A with a distinct multi-line payload → assert
     the echoed body matches exactly
   - Fire a tunnel-A request and a tunnel-B request concurrently
     (`tokio::join!`) → assert each still gets back only its own server's
     body, checking the two routes stay independently correct under
     concurrent use, not just sequential use.

   Everything downstream of this in the test (the `active_tunnels` map
   assertions, client-disconnect cleanup check) stays as-is for tunnel A —
   it already covers the two-profile/two-key bridging semantics this test is
   about — and gets a lightweight parallel assertion for tunnel B's entry
   (checking `target_entity_id == server_entity_b.id`) so the routing
   distinction is verified in the `active_tunnels` map too, not just over
   the wire.

4. Update the file's top doc comment and `FIXTURE_BODY`-related comments
   minimally to reflect the new varied-request coverage.

No changes needed to `tunnel2tunnel-ssh` or `tunnel2tunnel-core` — this is a
test-only change; the tunnel already transparently bridges arbitrary bytes,
so no product code should need touching.

## Verification

- `cargo test -p t2t --test tunnel_e2e -- --nocapture` against the local
  Postgres 18 instance (per `CLAUDE.md`/`MANUAL_TESTING.md` setup), requires
  a real `ssh` binary in `PATH` (test already skips itself if absent).
- Confirm all new assertions (`/alpha`, `/beta`, `/echo` echo, concurrent
  fetch) pass, and that deliberately breaking one route's response — locally,
  temporarily — fails the corresponding assertion, to confirm it isn't
  vacuously passing.
