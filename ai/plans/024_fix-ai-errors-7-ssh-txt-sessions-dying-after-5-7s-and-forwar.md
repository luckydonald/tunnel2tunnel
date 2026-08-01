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

`resolve_target_entity` (`crates/tunnel2tunnel-ssh/src/lib.rs:1428`) only accepts a UUID or an `entity_access.hostname` alias — never a raw entity name. `frontend/src/components/SshCommandDisplay.vue:60` builds the `-L` command's target host as `s.ownerName ?? s.ownerId`, i.e. it hands the user the entity's *display name* ("m1n") as the SSH forward host, which the backend currently can never resolve — a raw UUID is unreadable and not what the user wants to keep typing/seeing, so per the user's direction this is fixed on the **backend**: teach `resolve_target_entity` to also accept a plain entity name, so the already-correct frontend output starts working.

`entities.name` (`migrations/002_entities_keys_ports.sql:5`) is a plain nullable `TEXT` column with **no uniqueness constraint**, global or per-user (confirmed — no migration ever adds one, and `Entity::create`/`update` in `crates/tunnel2tunnel-core/src/models/entity.rs` never check for collisions). Two unrelated entities (even across different users) can legitimately share a name. Resolving by name therefore has to be done carefully:
- Match only against `entities.name` where `deleted_at IS NULL` (respect soft-delete like every other `Entity` query).
- If the name matches **more than one** entity, treat it as unresolved (`None`) rather than guessing — silently routing to an arbitrary same-named entity would be a correctness/security footgun. This mirrors the existing fail-closed behavior for a hostname that doesn't resolve at all.
- This does not change the security boundary: resolving a name to a `target_entity_id` only identifies which entity the client is *asking* for — the real authorization decision still happens exactly as it does today, via `PortConfig::find_enabled_by_entity_and_proxy_port` + `EntityAccess::check_access` right after resolution (`crates/tunnel2tunnel-ssh/src/lib.rs:1109` onward). A resolved-by-name target that the client has no grant for is rejected there, same as a resolved-by-UUID one. This also matches the existing `find_entity_by_hostname` alias lookup, which likewise isn't scoped to the requesting client — consistent with existing precedent, not a new pattern.

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

2. **`crates/tunnel2tunnel-core/src/models/entity.rs`** — add `Entity::find_unique_id_by_name(pool: &PgPool, name: &str) -> Result<Option<Uuid>, CoreError>`:
   ```rust
   pub async fn find_unique_id_by_name(pool: &PgPool, name: &str) -> Result<Option<Uuid>, CoreError> {
       let ids: Vec<Uuid> = sqlx::query_scalar(
           "SELECT id FROM entities WHERE name = $1 AND deleted_at IS NULL",
       )
       .bind(name)
       .fetch_all(pool)
       .await
       .map_err(CoreError::Sqlx)?;
       Ok(match ids.as_slice() {
           [id] => Some(*id),
           _ => None, // no match, or ambiguous (>1) — don't guess
       })
   }
   ```

3. **`crates/tunnel2tunnel-ssh/src/lib.rs:1428`** — extend `resolve_target_entity` to try this after the existing UUID and hostname-alias attempts:
   ```rust
   async fn resolve_target_entity(pool: &PgPool, hostname: &str) -> Option<Uuid> {
       if let Ok(id) = hostname.parse::<Uuid>() {
           return Some(id);
       }
       if let Ok(Some(id)) = EntityAccess::find_entity_by_hostname(pool, hostname).await {
           return Some(id);
       }
       Entity::find_unique_id_by_name(pool, hostname).await.unwrap_or(None)
   }
   ```
   `Entity` is already imported in this file (`tunnel2tunnel_core::models::entity::Entity`, used a few lines below in the same function's caller).

4. **`ai/errors/7.t2t.md`** (new file) — sanitized excerpt of the server-log evidence that ties the two bugs together, with all real IPs/UUIDs/hostnames replaced by obvious placeholders (e.g. `entity_id=<redacted-uuid>`, `peer_ip=203.0.113.10`, `entity_name="ExampleClient"`, host aliases like `"example-host"`). Include:
   - One connect → auth → `SshEncoding: length invalid` close cycle (timestamps relative or redacted).
   - One `direct-tcpip: rejected — target hostname not found` line.
   - A one-paragraph note on what each indicates and pointing at the two code fixes above (compression disabled; name-resolution added).

## Verification

- `cargo build -p tunnel2tunnel-ssh` (and `cargo check -p t2t`) to confirm the `Config`/`Preferred` wiring compiles against russh 0.61.2's actual public API (confirm the exact import path for `Preferred` — it's re-exported as `russh::Preferred` per `lib_inner.rs`, so `russh::Preferred` should work without reaching into `russh::negotiation`).
- `cargo build -p tunnel2tunnel-core -p tunnel2tunnel-ssh` to confirm `find_unique_id_by_name` and the updated `resolve_target_entity` compile.
- Manual SSH test against a locally-run server: connect with `ssh -o Compression=yes -p 2222 ...` and confirm the negotiated compression is `none` (via `ssh -v` output) and the session survives past the `ServerAliveInterval` keepalive window instead of dying with `SshEncoding: length invalid`.
- Manual SSH test: with two entities set up (one owning a service, one subscribed), run the `-L bindPort:<ownerName>:proxyPort` command exactly as shown in the UI and confirm the tunnel connects instead of logging `target hostname not found`. Also test the ambiguous case (two entities sharing a name) and confirm it's correctly rejected as not-found rather than routed to the wrong one.
- Frontend is unchanged, so no frontend test/build step is needed for this fix.

## Commit workflow (per user instruction, not the default lplp flow)

1. Commit normally (no `--amend`) once the fix + test + sanitized log file are done.
2. Run `./scripts/tag_backup.py`.
3. Interactively rebase to squash/reorder commits as makes sense (fold auto-commit hook commits per the lplp skill rules as usual).
4. If any rebase step needs to reset a branch pointer, use `--keep` instead of `--hard`.
