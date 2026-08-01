# Fix `ai/errors/7.ssh.txt`: sessions dying after ~5-7s, and forwards to the wrong hostname

## Context

`ai/errors/7.ssh.txt` shows a client (`Fedora Work`) that authenticates fine, gets the chat welcome banner, and then has its connection abruptly closed by the server ("Connection to ... closed by remote host") a few seconds later. Cross-referencing the server logs (`/home/user/Downloads/tunnel2tunnel-...-all-logs-2026-08-01-08-33-33.txt`) shows this is not an isolated glitch: from ~08:15 onward, essentially every authenticated session from this client is torn down 5-40s after auth with:

```
WARN tunnel2tunnel_ssh: SSH: session ended with error err=SshEncoding: length invalid
```

The client only opens a `client-session` channel (the chat/welcome channel) in this transcript — no forwarding traffic was even flowing yet — so the failure lives in the base session/channel machinery, not in the tunnel-bridging code. `SshEncoding: length invalid` is `ssh_encoding::Error::Length`, which `russh` 0.61.2 surfaces from `String::decode`/`u32::decode` calls while parsing incoming packets (e.g. `GLOBAL_REQUEST`, used for things like the client's `keepalive@openssh.com` pings — the client config here uses `ServerAliveInterval=3`, so those start arriving a few seconds in). The client and server negotiated `zlib@openssh.com` — the RFC 4253 "delayed" compression variant that only turns on after auth completes — which is exactly the point where the corruption starts appearing. This points at `russh`'s delayed-zlib compression path, not at our code's packet framing (we never hand-rolled protocol bytes; channel writes all go through `Handle::data`/`Session::channel_success`, which are safe). Since we can't patch the vendored `russh` crate, the practical fix is to stop negotiating zlib at all: `russh::server::Config::preferred.compression` defaults to `[none, zlib, zlib@openssh.com]` (all algorithms `russh` compiles in via its default `flate2` feature), and our server never overrides it. Restricting the server's preferred list to `[none]` makes negotiation always land on "none" and removes the buggy code path entirely, at the cost of some bandwidth (irrelevant for this tool's traffic).

Separately, the log also shows every `direct-tcpip` attempt from this client failing:

```
INFO tunnel2tunnel_ssh: direct-tcpip: rejected — target hostname not found (not a UUID or known alias) host="m1n" port=5900
```

`resolve_target_entity` (`crates/tunnel2tunnel-ssh/src/lib.rs:1428`) only accepts a UUID or an `entity_access.hostname` alias — never a raw entity name. But `frontend/src/components/SshCommandDisplay.vue:60` builds the `-L` command's target host as `s.ownerName ?? s.ownerId` — i.e. it hands the user the entity's *display name* ("m1n") as the SSH forward host, which the backend can never resolve. The UI is generating a command that is guaranteed to fail for any subscription whose owner entity has a name set (which is normal). The fix is to always use the UUID (`s.ownerId`), which `resolve_target_entity` resolves unconditionally.

## Changes

1. **`crates/tunnel2tunnel-ssh/src/lib.rs`** — in `start()`, override `Config::preferred` to disable compression:
   ```rust
   use russh::negotiation::Preferred; // or russh::Preferred, whichever path is public
   use russh::compression;
   ...
   let russh_config = Arc::new(Config {
       keys: vec![key],
       preferred: Preferred {
           compression: std::borrow::Cow::Borrowed(&[compression::NONE]),
           ..Preferred::default()
       },
       ..Config::default()
   });
   ```
   Add a short comment explaining *why* (russh 0.61.2's delayed `zlib@openssh.com` compression corrupts post-auth packets — observed as sessions dying seconds after connecting with `SshEncoding: length invalid`; see `ai/errors/7.t2t.md`).

2. **`frontend/src/components/SshCommandDisplay.vue:60`** — change the subscribed-row `toHost` from `s.ownerName ?? s.ownerId` to `s.ownerId`, so the generated `-L` flag always targets a hostname the backend's `resolve_target_entity` (UUID-or-alias) can actually resolve.
   - Update `frontend/src/components/SshCommandDisplay.spec.ts:206` — the existing assertion `expect(cmdText).toContain('-L 5901:home-nas:5900')` currently encodes the buggy behavior; change it to expect `-L 5901:home-nas-id:5900` (the `ownerId` from the test's mock data).

3. **`ai/errors/7.t2t.md`** (new file) — sanitized excerpt of the server-log evidence that ties the two bugs together, with all real IPs/UUIDs/hostnames replaced by obvious placeholders (e.g. `entity_id=<redacted-uuid>`, `peer_ip=203.0.113.10`, `entity_name="ExampleClient"`, host aliases like `"example-host"`). Include:
   - One connect → auth → `SshEncoding: length invalid` close cycle (timestamps relative or redacted).
   - One `direct-tcpip: rejected — target hostname not found` line.
   - A one-paragraph note on what each indicates and pointing at the two code fixes above.

## Verification

- `cargo build -p tunnel2tunnel-ssh` (and `cargo check -p t2t`) to confirm the `Config`/`Preferred` wiring compiles against russh 0.61.2's actual public API (confirm the exact import path for `Preferred` — it's re-exported as `russh::Preferred` per `lib_inner.rs`, so `russh::Preferred` should work without reaching into `russh::negotiation`).
- Manual SSH test against a locally-run server: connect with `ssh -o Compression=yes -p 2222 ...` and confirm the negotiated compression is `none` (via `ssh -v` output) and the session survives past the `ServerAliveInterval` keepalive window instead of dying with `SshEncoding: length invalid`.
- `cd frontend && npm run test:unit` (or the project's actual vitest script) to confirm the updated `SshCommandDisplay.spec.ts` passes with the `ownerId`-based assertion.
- Manually eyeball the rendered SSH command in the Entity Detail page for an entity with an enabled subscription, confirming the `-L` flag now shows the owner's UUID instead of its name.

## Commit workflow (per user instruction, not the default lplp flow)

1. Commit normally (no `--amend`) once the fix + test + sanitized log file are done.
2. Run `./scripts/tag_backup.py`.
3. Interactively rebase to squash/reorder commits as makes sense (fold auto-commit hook commits per the lplp skill rules as usual).
4. If any rebase step needs to reset a branch pointer, use `--keep` instead of `--hard`.
