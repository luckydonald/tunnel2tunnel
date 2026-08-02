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
all the required scaffolding) so it drives several distinct HTTP requests
through the same established tunnel and asserts each gets back the correct,
distinct response.

## Approach

All changes are in `crates/t2t/tests/tunnel_e2e.rs`.

1. **Generalize the fixture service** (`spawn_fixture_service`, currently
   ignores the request and always replies with `FIXTURE_BODY`). Change it to
   a minimal request-aware responder:
   - Parse just the request line (method + path) out of the raw bytes it
     already reads.
   - Table of routes, each returning a distinct fixed body:
     - `GET /alpha` → `"alpha response body"`
     - `GET /beta` → `"beta response body"` (different length than alpha, to
       catch any hardcoded `Content-Length` bugs)
     - `POST /echo` with a request body → echoes the body back verbatim (this
       exercises client→server bytes flowing through the tunnel, not just
       server→client, since the existing test only ever sent a bare GET with
       no body)
     - keep the original `/` → `FIXTURE_BODY` route so the first existing
       assertion (proving the pipe comes up at all) is unchanged
   - Anything else → `404`, to make an unrouted request an obvious failure
     rather than a silent success.

2. **Generalize `fetch_through_tunnel`** into something like
   `fetch_through_tunnel(port, method, path, body, timeout)` that builds the
   appropriate request bytes and returns the raw response, keeping the same
   "retry until it succeeds or timeout" structure (still needed for the first
   request, since it races the `-R`/`-L` handshake). Requests after the first
   successful one don't need the retry loop (the tunnel is already up) but
   reusing the same helper is simpler and harmless.

3. **Replace the two identical `fetch_through_tunnel` calls** with a
   sequence exercising the new routes over the *same* already-established
   `local_port` tunnel:
   - `GET /` (unchanged — keeps proving first-contact works)
   - `GET /alpha` → assert `alpha response body`
   - `GET /beta` → assert `beta response body` (and assert it does **not**
     equal the alpha body, guarding against a stuck/cached response)
   - `POST /echo` with a distinct payload (e.g. including some binary-ish or
     multi-line content) → assert the echoed body matches exactly
   - Fire two of the GET requests concurrently (`tokio::join!`) as a light
     check that the tunnel correctly multiplexes overlapping direct-tcpip
     channels rather than serializing/corrupting them.

   Everything downstream of this in the test (the `active_tunnels` map
   assertions, client-disconnect cleanup check) stays as-is — it already
   covers the two-profile/two-key bridging semantics this test is about.

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
