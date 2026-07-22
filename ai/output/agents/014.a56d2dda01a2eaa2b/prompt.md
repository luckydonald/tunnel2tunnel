Design a detailed implementation plan for a new feature in the tunnel2tunnel repo (self-hosted SSH rendezvous manager, Rust workspace: tunnel2tunnel-core / tunnel2tunnel-web / tunnel2tunnel-ssh / t2t binary / Vue3 frontend). Read CLAUDE.md at repo root first for architecture/conventions.

## Goal
Combine fail2ban-style banning with SSH tarpit techniques (like endlessh) directly into the existing russh server (crates/tunnel2tunnel-ssh/src/lib.rs), to slow down/waste resources of brute-force SSH login scanners hitting the rendezvous server, while never tarpitting legitimate clients.

## Confirmed design decisions (from user, do not re-litigate)
1. Extend the existing `connection_logs` table/model rather than create a new one — but tighten `failure_reason` (currently free TEXT with no CHECK) into a proper constrained enum-like column following the repo's existing enum pattern (TEXT + CHECK constraint in migration, plain String in Rust struct, runtime validation array in web handler, TS string-literal union + labels.ts Record/Options in frontend) as used for `subject_type`/`visibility_grant` in entity_access/friendships.
2. Ban/tarpit triggering uses admin-configurable global thresholds (e.g. "N failed attempts within window X" applied per peer_ip and per user/entity), editable via the webui.
3. Admin-created ban/tarpit rules (by peer_ip or by user) support an optional expiry ("active until"), indefinite if blank — mirror the existing `valid_until` pattern used on entities/ssh_keys.
4. Three round-robined tarpit methods for v1:
   a. Endlessh-style banner drip: endless stream of random RFC4253-legal "other lines of data" before ever sending the real SSH identification string, so the client hangs forever during the pre-auth banner phase.
   b. Slow auth drip-feed: accept the TCP connection normally but introduce artificial delays during the username/password or publickey auth exchange (e.g. delay each auth reply).
   c. Fake interactive shell: let the attacker "succeed" just far enough to reach a bogus shell prompt that then hangs / echoes garbage forever, never granting real access.

## Required behavior/spec details from the user's original request (must all be covered by the plan)
- This is fail2ban-like: tarpitting/banning should only kick in after wrong attempts (never tarpit on a client's very first/legitimate attempt).
- A valid, successful publickey login (see example log sequence below) must NEVER be tarpitted or blocked, even if that peer_ip or user was previously banned/tarpitted for other failed attempts — a successful login should clear/override any active ban state for that identity for future connections (their next login attempt should NOT need to fight through the tarpit) is NOT required — but this specific already-successful session must not be interrupted. Read the "success clears ban" wording carefully in the fail/success reason design below and resolve the ambiguity in your plan (state your resolution explicitly).
- Rules must be settable via the website by an admin: by peer_ip OR by user (which "user" — the entity id? the t2t account/User model? Investigate and decide — entities authenticate over SSH via pubkey, not "users" logging in with a username, so figure out what "user" means in this SSH context and document your reasoning).
- Every auth attempt (successful or not) must be logged with: peer_ip; user (if the SSH auth flow got far enough to identify one — see existing auth_publickey flow which resolves a fingerprint to an entity_id); if a password was attempted, log the attempted password value itself (yes, store the raw attempted password — that's intentional per the user's request, since password auth isn't even a valid method here, it's just attacker recon); if a public key was attempted, store its fingerprint under a clearly-named column (not just "fp" abbreviation — something like `key_fingerprint`, matching the existing connection_logs column name); which tarpit method (if any) was applied to this connection.
- The log table needs a computed/derived boolean "success" column based on two mutually-exclusive nullable "reason" column pairs:
  - "why it failed" reason (free enum: e.g. password attempted or unsupported auth attempted, wrong user/entity not found, wrong ssh key/unknown key, IP whitelist blocked, key/entity expired, tarpitted-and-then-gave-up, etc.) — nullable, must be NULL when success.
  - "why it succeeded" reason (free enum: e.g. correct login, admin reset the ban in the webui, etc.) — nullable, must be NULL when failed.
  - Exactly one of these two reason columns must be non-NULL per row (design this as a CHECK constraint), and the "success" boolean is derived from which one is set (either a GENERATED ALWAYS AS column, or computed at read time — pick one and justify it against the existing sqlx::FromRow model pattern in this codebase).
- This connection_logs table (extended) must be admin-browsable in the webui with pagination, filtering, and search — there is currently NO pagination/filter/search anywhere in this codebase (existing `list_connection_logs` route only takes a fixed LIMIT 50, no query params) so this must be designed from scratch. Look at the existing Access Rules CRUD UI in frontend/src/pages/EntityDetailPage.vue (data-table + add-rule-form + delete-button pattern) as the closest existing precedent to reuse stylistically, but the log browser itself needs new pagination/filter/search — propose a simple, idiomatic design (e.g. `?page=&page_size=&peer_ip=&user=&success=&method=&q=` query params, offset-based pagination, ILIKE-based search) consistent with Axum 0.8 + SQLx 0.8 conventions already used in this repo.
- Must write fail2ban-compatible log lines (matching the existing sshd-style format already written by `log_auth_failure`/`log_auth_success` in tunnel2tunnel-ssh/src/lib.rs, and matched by contrib/fail2ban/filter.d/tunnel2tunnel.conf) — extend this so tarpit-triggering events are captured in that log too, and update the fail2ban filter regex if new line formats are introduced.
- The known-good legitimate login sequence must NEVER trigger blocking. Reference this real log sequence (already captured in ai/errors/4.legitimiate.txt) as the canonical "must not be tarpitted" case:
```
SSH: new connection peer_ip=X
SSH: auth attempt (none) — rejected user=<entity_uuid> available="publickey"
SSH: publickey offered (probe, no signature yet) user=<entity_uuid> fp=SHA256:...
SSH: auth attempt (publickey) user=<entity_uuid> fp=SHA256:...
SSH: auth accepted entity_id=<entity_uuid> entity_name="Laptop"
SSH: connection closed (authenticated)
```
  Note the "auth attempt (none) — rejected" step is a normal, required part of every legitimate SSH handshake (the client always probes with `none` auth first) — the ban-threshold logic must not count this `none`-method probe as a real "failed attempt" toward any ban threshold, only real credential/key rejections should count. Explicitly call this out in the plan as a specific implementation detail/pitfall to get right, and specify exactly which of the existing rejection code paths in `auth_publickey`/`auth_password` (lines documented below) should vs should not increment a failure counter.
- Every method (each of the 3 tarpit techniques, the threshold/ban-rule engine, the "success clears active tarpit state for this connection" rule, the log write-path, the fail2ban line format, and the admin API/UI) needs automated tests. Additionally there must be a specific unit/integration test asserting that the exact legitimate-login sequence above does NOT get tarpitted/blocked.

## Existing code context already gathered (do not re-explore these — treat as ground truth; verify only if something seems off)

### crates/tunnel2tunnel-ssh/src/lib.rs (single file, ~940 lines)
- `T2tServer` (line 111-116): `pool: PgPool`, `server_slots: ServerSlots` (`Arc<Mutex<HashMap<(Uuid,u32),(Handle,String)>>>`), `session_registry: SessionRegistry` (`Arc<Mutex<HashMap<Uuid, SessionEntry>>>`), `fail2ban: Option<Arc<String>>`.
- `T2tHandler` (line 151-161, one per TCP connection, built in `new_client` line 121-138): `pool`, `server_slots`, `session_registry`, `fail2ban`, `conn_id: Uuid`, `peer_ip: String`, `entity: Option<AuthedEntity>`, `bridges: HashMap<ChannelId,(Handle,ChannelId)>`, `log_id: Option<Uuid>`.
- `AuthedEntity` (143-147): `{ entity: Entity, user_id: Uuid }`.
- `impl Handler for T2tHandler`: `auth_none` (234-245), `auth_password` (247-260, always rejects — password auth not supported, but still worth logging attempted password per this feature's spec), `auth_keyboard_interactive` (262-279, always rejects), `auth_publickey_offered` (281-296, accepts all probes), `auth_publickey` (298-394 — real logic: unknown key line 314/reject; db error line 319; key soft-deleted line 329/reject; key valid_until expired line 337/reject; entity not found line 347/reject; entity db error line 352; entity expired line 362/reject; IP whitelist blocked line 371-377/reject; success line 382-391), `auth_succeeded` (397-482, must `tokio::spawn` any `Handle` confirmation calls per CLAUDE.md gotcha — never `.await` inline, deadlocks), `channel_open_session` (485-526, currently early-returns `Ok(false)` if `self.entity.is_none()` at line 490 — a fake-shell tarpit needs to either bypass this guard under a tarpit flag or introduce a `SessionKind` enum distinguishing `Authed`/`Tarpit`), `tcpip_forward`/`cancel_tcpip_forward`/`channel_open_direct_tcpip`/`data`/`channel_eof`/`channel_close`.
- `impl Drop for T2tHandler` (776-826): cleans up session_registry/server_slots, calls `ConnectionLog::set_ended`.
- Logging methods on `T2tHandler`: `log_auth_failure(&self, fingerprint: Option<&str>, reason: &str)` (164-197) — writes a `ConnectionLog::create(...)` row AND (if `self.fail2ban` set) appends an sshd-format "Failed publickey for invalid user {fp} from {peer_ip} port 0 ssh2" line via `append_to_file`. `log_auth_success(&mut self, entity: &Entity, fingerprint: &str)` (199-228) — same but "Accepted publickey for {entity_id} from {peer_ip} port 0 ssh2", sets `self.log_id`.
- Helpers: `chrono_like_timestamp()` (917-929, sshd-style `"Mon  D HH:MM:SS"`), `append_to_file(path, content)` (931-940, tokio append-mode file write).
- `SshConfig` (29-36): `ssh_port`, `fail2ban_log_path: Option<String>`, `host_key_path`, `host_key_password: Option<String>` — env vars wired in `crates/t2t/src/main.rs` (fail2ban_log_path from `FAIL2BAN_LOG_PATH` at line 39).
- No rate-limiting/ban state exists anywhere today — every `T2tHandler` is stateless/per-connection, nothing persists across connections in-process. `server_slots`/`session_registry` are the existing precedent for `Arc<Mutex<HashMap<...>>>` shared state threaded through `T2tServer` → `new_client` → `T2tHandler`, use the same wiring pattern for new tarpit/ban shared state.
- `channel_open_session`'s existing "Welcome" banner + periodic-ping-via-`tokio::select!`-racing-`channel.wait()`-against-`tokio::time::interval` pattern (lines ~442-464) is a good template to copy for the fake-shell tarpit's hang-forever loop.

### tunnel2tunnel-core lookups used by the SSH handler
- `SshKey::find_by_fingerprint(pool, fp)` — `crates/tunnel2tunnel-core/src/models/ssh_key.rs:25-36`, `WHERE fingerprint = $1 AND deleted_at IS NULL`.
- `Entity::find_by_id_only(pool, id)` — `entity.rs:43-54`, `WHERE id = $1 AND deleted_at IS NULL`.
- `ip_whitelist::evaluate(rules: &str, peer_ip: &str) -> bool` — `crates/tunnel2tunnel-core/src/ip_whitelist.rs`, pure/stateless, newline rules, CIDR/glob/regex/`!`-deny, first-match, empty=allow-all. Reuse this exact evaluator for peer_ip-based ban rule matching if it fits (it may — a ban rule is conceptually a deny-list of peer_ips), or note why a separate simpler mechanism is warranted (e.g. because ban rules need per-rule expiry + reason + admin CRUD, unlike whitelist which is a single text blob per entity).
- `Entity` struct: `id, user_id, entity_type, name, description, ip_whitelist: Option<String>, valid_until: Option<OffsetDateTime>, ts: TimestampsSoftDelete`. Note **`Entity.user_id`** exists — this may be the answer to "what does 'user' mean in ban-by-user context", but investigate: does the SSH auth flow ever know the `User` (human account) before/without resolving through an `Entity`? Check `crates/tunnel2tunnel-core/src/models/user.rs` and decide whether ban-by-"user" means ban-by-entity_id, ban-by-owning-User.id, or both should be supported, and justify your choice given that unauthenticated attackers are never tied to a `User` until a valid key fingerprint resolves through `SshKey`→`Entity`→`user_id`.

### connection_logs (to be extended, per confirmed decision)
`migrations/004_connection_logs.sql`:
```sql
CREATE TABLE connection_logs (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    entity_id       UUID REFERENCES entities(id) ON DELETE SET NULL,
    peer_ip         TEXT,
    key_fingerprint TEXT,
    login_succeeded BOOL NOT NULL,
    failure_reason  TEXT,
    ssh_flags       TEXT,
    ports_requested TEXT,  -- dead column, never read/written outside this migration
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at        TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON connection_logs FOR EACH ROW EXECUTE FUNCTION set_timestamps();
CREATE INDEX connection_logs_entity_id_idx ON connection_logs(entity_id);
CREATE INDEX connection_logs_started_at_idx ON connection_logs(started_at DESC);
```
Rust model `crates/tunnel2tunnel-core/src/models/connection_log.rs`: `ConnectionLog { id, entity_id: Option<Uuid>, peer_ip: Option<String>, key_fingerprint: Option<String>, login_succeeded: bool, failure_reason: Option<String>, ssh_flags: Option<String>, ports_requested: Option<String>, started_at, ended_at: Option<OffsetDateTime>, #[sqlx(flatten)] ts: Timestamps }`. Fns: `create(...)` (26-55, `RETURNING *`), `set_ended(pool,id)` (57-64), `list_for_entity(pool, entity_id, limit)` (66-80, only scoping/limit, no pagination/filter/search).
Web route `crates/tunnel2tunnel-web/src/routes/entities.rs` `list_connection_logs` (535-573) + `ConnLogResponse` DTO, registered at `crates/tunnel2tunnel-web/src/lib.rs:101-102` as `GET /api/entities/{id}/logs`.
Frontend: `frontend/src/api/admin.ts` `ConnLog` interface (11-19) + `listConnectionLogs` (105-106); `frontend/src/pages/EntityDetailPage.vue` "Connection Log" section (293-309 script, 645-678 template) — lazy-loaded via `logsLoaded` boolean gate, plain table, "Refresh" button, no pagination/filter/search.

### Existing enum pattern to replicate for new "tarpit method" / "fail reason" / "success reason" columns
DB: `TEXT NOT NULL CHECK(col IN (...))`, e.g. `migrations/003_access_friends.sql` `subject_type TEXT NOT NULL CHECK(subject_type IN ('entity','all_mine','all_user_entities','public_lite'))`. Rust: plain `String` field, no Rust enum, runtime-validated in the web handler against a literal `&[&str]` array (see `crates/tunnel2tunnel-web/src/routes/access.rs:76-95`: `let valid_types = ["entity","all_mine",...]; if !valid_types.contains(&body.subject_type.as_str()) { return Err(WebError::BadRequest(...)); }`). TypeScript: string-literal union in the relevant `api/*.ts` interface (e.g. `frontend/src/api/friends.ts:6,21`). Labels: `frontend/src/labels.ts` — `Record<Union,string>` map + derived `Options` array via `Object.entries(...).map(([value,label])=>({value,label}))`, e.g. `subjectTypeLabel`/`subjectTypeOptions` (lines 1-13).

### Access Rules CRUD UI precedent to copy stylistically for the new admin ban-rules editor
`frontend/src/pages/EntityDetailPage.vue` lines 171-277 (script) / 514-617 (template): toggleable "Add rule" form with a `<select>` (fed by an `xxxOptions` array) + conditional sub-fields + free-text field, `data-table` listing existing rules each with a `×` delete button, `blank...()` factory function for the form's reset state, `handleAdd...`/`handleDelete...` handlers, lazy-load-on-demand via boolean gate. Backed by `entity_access` table + `crates/tunnel2tunnel-core/src/models/entity_access.rs` (`check_access` query at lines 96-123 — access rules use `subject_type` matching against `'public_lite'|'all_mine'|'all_user_entities'|'entity'`).

### fail2ban integration
`contrib/fail2ban/filter.d/tunnel2tunnel.conf`:
```
[Definition]
failregex = ^.+ t2t sshd\[\d+\]: Failed publickey for invalid user .+ from <HOST> port \d+ ssh2$
ignoreregex =
```
Only matches the "Failed" line; "Accepted" lines are written but not matched by anything (informational only). New tarpit-triggered lines need their own format + (if fail2ban should act on them) a corresponding filter regex update — decide whether tarpit-trigger events should feed fail2ban bans too, or whether tarpit and fail2ban are meant as two independent/parallel defenses (this repo's own ban-rule engine handles peer_ip/user-scoped banning going forward, separate from any external fail2ban process) — state your reasoning.

### Existing test patterns
Only integration test: `crates/t2t/tests/tunnel_e2e.rs` (297 lines) — uses **real OpenSSH client subprocesses** (`Command::new("ssh")`), not a Rust SSH client library (`russh::client` is never imported anywhere in the repo) — this is a deliberate choice per the test's own doc comment (only real OpenSSH reproduces certain forwarded-tcpip matching behavior). Provides reusable fixtures: DB bootstrap + `sqlx::migrate!` (158-167), scratch dir per Uuid (169-175), `generate_test_keypair()` (79-94), `free_port()` (48-54), server startup + poll-until-listening (219-235), `ChildGuard` (58-64, kills child on Drop), `log_child_stderr` (68-77), subprocess-based SSH login attempts (246-257, 266-279). Currently no test ever sets `fail2ban_log_path: Some(...)` (always `None`) — no existing coverage of the fail2ban file-writing path at all. Unit tests only otherwise exist in `tunnel2tunnel-core/src/pubkey.rs` and `.../ip_whitelist.rs` (`mod tests` blocks). For the new feature's per-method tests (banner drip, slow auth drip, fake shell, threshold engine, success-clears-tarpit rule, fail2ban line format, admin API/UI, and the specific legit-login-sequence-must-not-tarpit test), decide for each whether a fast Rust unit test (no network) suffices vs. whether it needs the heavier OpenSSH-subprocess e2e harness, and justify — prefer unit-testable pure logic (threshold counter, rule matching, DTO validation) isolated from the network/actor code wherever feasible so most tests don't need spawning real processes.

## What the plan must include
1. Full migration SQL (new migration file, e.g. `008_tarpit.sql`) — extended connection_logs columns, CHECK constraints (including the mutual-exclusivity constraint on the two reason columns and how "success" is derived — GENERATED column vs computed-at-read — state your decision and reasoning), new admin-manageable ban-rules table (peer_ip or user scope, optional expiry, reason/note field, created_by admin), any new settings for admin-configurable global thresholds (reuse the existing but currently-unused key-value `settings` table? investigate whether it's worth reviving it here, or add dedicated columns/rows — decide and justify).
2. Core model changes/additions in tunnel2tunnel-core (structs, query fns including the new pagination/filter/search query for connection_logs, ban-rule CRUD fns, threshold-counting query/logic).
3. tunnel2tunnel-ssh changes: new shared tarpit/ban state threaded through T2tServer/T2tHandler (mirroring server_slots/session_registry wiring), the three tarpit method implementations, round-robin selection logic, hook points in auth_publickey/auth_password/auth_none (being precise about which rejection reasons increment failure counters and which don't — especially not counting the `none`-probe), the "success clears this connection's active-tarpit-consideration" resolution, fail2ban line writing for tarpit events.
4. tunnel2tunnel-web: new/extended routes for paginated/filterable/searchable connection log browsing, and admin CRUD routes for ban rules and threshold settings.
5. Frontend: new admin page(s)/section(s) for the log browser (pagination/filter/search UI) and ban-rules editor, plus labels.ts additions, following existing patterns exactly.
6. Test plan: enumerate concrete test cases per component (unit vs e2e, which existing fixtures to reuse), explicitly including the legit-login-sequence non-tarpit test.
7. List exact files to create/modify with line-anchored pointers where you're modifying existing files (not necessarily final line numbers, but "near line X in function Y").
8. Flag any open design ambiguities you had to resolve and state your resolution + reasoning for each (especially: ban-by-"user" semantics, success-clears-ban semantics, fail2ban vs internal-ban-engine relationship, GENERATED column vs app-level success computation).

Do not write any code yet — produce a structured, detailed implementation plan document (as your final report/answer) covering all of the above. Be concrete about function signatures, SQL, and file locations, not just prose descriptions.