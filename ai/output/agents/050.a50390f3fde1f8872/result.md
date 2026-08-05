I now have everything needed. Let me compile the final report.

# Research Report

## 1. SSH-side chat feature (relay, char-by-char input, keepalive ping)

**File:** `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs`

- `SessionRegistry` / `broadcast()` — lines 176–202. `SessionEntry { handle, session_channel_id: Option<ChannelId> }`, keyed by connection `Uuid`. `broadcast()` (187–202) sends a formatted string to every registered session's `session_channel_id`, optionally excluding one connection.
- `channel_open_session` — lines 935–1001. When a client opens an interactive session channel (no `-N`), it's registered as `session_channel_id` (957–961), a welcome banner is sent (967–974, tells the user "Type a message and press Enter to chat"), then a background task (977–998) is spawned that both watches for channel close/EOF via `ch.wait()` and, on a `tokio::time::interval(5*60s)`, sends the Derpy Hooves keepalive ping text (line 991: `"\r\n\x1b[33m✉ Derpy Hooves stopped by to make sure your tunnel is still up! 🧁\x1b[0m\r\n\r\n"`) via `ping_handle.data(ch_id, ...)`. This is the reusable formatting pattern (ANSI color code + emoji + `\r\n` framing) for any new chat-timestamp formatting.
- `data()` handler — lines 1548–1585. This is the Handler-trait callback that receives raw bytes from the client's session channel. Chat relay logic is at 1561–1582: it looks up whether the incoming `channel` matches the registered `session_channel_id` for `self.conn_id`; if so, it treats the *entire byte chunk* delivered to a single `data()` call as one line: `text.trim_end_matches(['\r','\n'])`, and if non-empty, formats + broadcasts it (see section 2). **There is no per-character input buffer** — no accumulation of bytes across multiple `data()` calls, no explicit "wait for Enter" state machine. It works today only because there's no server-side pty (`pty_request` at 1007 just ACKs via `channel_success` without actually allocating a pty — see comment at 1003–1006), so an interactive OpenSSH client is left in local canonical-echo mode and sends a full line (terminated by `\r`/`\n`) per `data()` invocation. This is exactly where "submit on enter" would need real handling if partial chunks/char-by-char delivery must be supported (e.g., a real pty, or clients that send raw unbuffered bytes) — currently any change to pty allocation or client terminal mode would break the "whole line arrives in one `data()` call" assumption.
- `shell_request` (1033–1040) and `exec_request`/`subsystem_request` (1055–1073) are no-ops/failures — no real shell is implemented, reinforcing that chat exists purely via the session channel's raw `data()` stream.

## 2. Chat message format lacking timestamps

**File:** same file, exact format string at **lines 1573–1578**:
```rust
let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)");
let short_id = &authed.entity.id.to_string()[..8];
let msg = format!(
    "\r\n\x1b[36m{} ({}): {}\x1b[0m\r\n\r\n",
    entity_name, short_id, text
);
broadcast(&self.session_registry, &msg, Some(self.conn_id)).await;
```
No timestamp is included anywhere in this string — only `entity_name (short_id): text`, wrapped in cyan ANSI (`\x1b[36m`) and blank-line padding. Related non-chat broadcasts using the same pattern-with-no-timestamp: connect message (lines 925–929, green `📡 {name} connected.`), port-registered/unregistered messages (lines 1141–1145 and 1174–1178, green/red 🟢/🔴).

## 3. `StatusDot.vue` — oval vs circle styling

**File:** `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/components/StatusDot.vue`, lines 30–55.

The `.status-dot` class (31–37) sets `display: inline-block`, `font-size: 0.8125rem`, `line-height: 1`, `padding: 0.1875rem`, and `border-radius: 50%` with a `box-shadow` ring — but there is **no explicit `width`/`height`** set. The element's box is sized entirely by its content (the emoji glyph from `dotEmoji()`, imported from `@/liveStatus`) plus the uniform padding. Since emoji glyphs are typically wider than they are tall (non-square advance width vs. line-box height), the bounding box is not square, so `border-radius: 50%` and the `box-shadow` ring render as an oval/pill rather than a true circle. The `ring-*` modifier classes (39–53) only vary `box-shadow` color, not shape.

## 4. Port badges / subscriber count on server-side services

**File:** `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/EntityDetailPage.vue`

- `serviceStatusMap` computed — lines 47–55: built from the shared live-status store (`entitySnapshot.value?.services`), keyed by `port_config_id`. Each entry has `.live`, `.remote_status`, and `.subscribers` (an array).
- Badge/count UI — lines 596–606: if `serviceStatusMap.get(port.id)?.subscribers.length` is truthy, renders a `<button class="btn-subscribers">` showing an expand/collapse triangle plus `{{ subscribers.length }} connected` (line 603); otherwise renders a plain `0` (`.td-desc`, line 605). This **already is** a notification-counter-style badge driven by `services[].subscribers[].length` exactly as hypothesized — no new data plumbing needed, it's live.
- Expanded detail row — lines 609–617: when expanded, lists each subscriber (`sub.entity.name`, `sub.account.username`, `sub.peer_ip`, `formatSince(sub.connected_since)`) in a `<ul class="subscriber-list">`.
- `StatusDot` usage for the same row is at lines 554–556 (separate live/remote-status indicator, distinct from the subscriber-count badge).

## 5. Hardcoded "localhost" remote host / PortConfig model

**File:** `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs`

- `tcpip_forward()` — lines 1076–1148. When a `-R` forward arrives with no existing enabled `port_configs` row for `(entity_id, port)`, it auto-creates one via `PortConfig::create(...)` (1104–1114) with the `host` argument **hardcoded as the literal string `"localhost"`** (line 1113), and `name` guessed from `guess_service_name(*port)` (line 1103, falls back to `"Unnamed Service"`).
- Importantly, this `host` field is **stored but not actually used** anywhere in the live bridging path. The real per-connection remote address comes from `registered_address` (the second element of the `server_slots` tuple, populated from the `address` parameter of `tcpip_forward` at line 1133 — i.e., whatever the *server-side SSH client* itself passed as the bind address in its own `-R address:port:...` invocation, typically `"localhost"`). This is used at connect time in `channel_open_direct_tcpip`'s spawned task, lines 1477–1482, via `server_handle.channel_open_forwarded_tcpip(&registered_address, ...)`; the comment there (1477–1480) explicitly notes this must match what the server's ssh client registered, not any UI-configured value.
- `guess_service_name` — `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/port_names.rs`, function starts line 7, a `match port { 21 => "FTP", 22 => "SSH", ... }` static lookup table by well-known port number only (no relation to entity/service naming).

**Model:** `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/port_config.rs`
- `PortConfig` struct, lines 13–29: fields `id, entity_id, enabled, local_port, proxy_port, name` (String — already a human name field, e.g. what shows in the frontend's "Name" column), `description`, `sort_order`, and `host` (line 25, doc comment: "The host to forward connections to (default: \"localhost\")... Lets you expose a service on another machine reachable from the owning entity."). So there **is** a dedicated `name` field distinct from `host`; `host` is the only "hostname to dial" field and is currently always `"localhost"` for auto-created rows, with no derivation from `name`.
- `create()`/`update()` (42–107) both take `name: &str` and `host: &str` as fully independent parameters — nothing today lowercases/derives `host` from `name`.

## 6. Ban/Trap threshold-rule validation (first-attempt trap/ban)

**Files:**
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/tarpit.rs`: `validate_threshold_body()` (lines 365–378) only checks `fail_count <= 0` → error (366–368), `window_seconds <= 0` → error, and `action` must be `"trap"`/`"ban"`. **Nothing prevents `fail_count == 1`** — a rule that trips/bans on the very first failed attempt is fully allowed to be created via `create_threshold`/`update_threshold` (routes at ~381–426).
- `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`: `record_failure()` (lines 174–221) increments a per-rule tally (195) and trips (197: `if tally.0 >= rule.fail_count.max(0) as u32`) — with `fail_count = 1` this trips on the *first* call. There's an existing unit test that documents this exact behavior as current, intended semantics: `record_failure_bans_on_very_first_attempt_when_fail_count_is_one` (lines 614–625, using `rule(1, 60)`), asserting the very first `record_failure` call returns banned=true.
- `record_auth_failure()` (lines 279–294) is called identically regardless of auth stage — from `lib.rs` line 491, invoked from `log_auth_failure` whenever `counts_toward_ban` is true. `auth_password()` in `lib.rs` (starts line 573) always rejects (password auth isn't supported at all) and calls `log_auth_failure(..., true)` (`counts_toward_ban=true`, around lines 581–589), meaning a **single incoming password attempt** counts toward the same fail tally as a bad publickey attempt — so a `fail_count=1` "trap"/"ban" rule bans/traps on the very first password-stage failure too, with no special-casing distinguishing publickey vs. password-stage failures anywhere in this file.

## 7. Collapsed summary / green `-L` command expand-to-name-field

**File:** `/home/user/git/luckydonald/tunnel2tunnel/frontend/src/components/SshCommandDisplay.vue`

- The SSH command block (lines 87–106) renders each `-L`/`-R` tunnel flag as a `<span class="cmd-flag">` (lines 98–103), generated from `allRows` (`ownedRows` + `subscribedRows`, computed at 47–68, each a `TunnelRow` with an `id`, `name`, and `flag` string like `-L 8080:hostname:443`). These spans currently only have `@mouseenter`/`@mouseleave` handlers (lines 100–102) that set `hoveredPortId`, used purely to highlight the matching row in the manual-config table below — **there is no `@click` handler** on these flag spans, so clicking one does nothing today.
- The collapsed "Manual configuration" summary is a native `<details>`/`<summary>` element (lines 109–113, `class="cmd-block"`/`"cmd-header manual-summary"`), closed by default (no `open` attribute), toggled only by native `<details>` click-to-expand — nothing in script wires the `-L`/`-R` chip click to opening it.
- The `name` field the todo references is already rendered inside that manual table once expanded: the "Name" column, line 231 (`<td>{{ row.name }}</td>`), part of the `manual-tunnel-table` (lines 202–234), each `<tr>` keyed by `row.id` and highlighted via the same `isHovered(row.id)`/`hoveredPortId` mechanism (lines 215–221, `70–72`). So the pieces (row-id linkage, name field, hover-highlight wiring) all exist — only "click chip → also set `details.open = true` and scroll/highlight that row" is unimplemented.
- No `PubkeyInput`-named component was found elsewhere in `frontend/src/components/` referencing this same summary/collapse behavior; `SshCommandDisplay.vue` is the sole owner of both the flag chips and the collapsible summary described in the todo.