## Findings

**Yes — the server unconditionally opens a server-initiated `session` channel back to every authenticated client**, and this fully explains the `client_input_channel_open: ctype session ... / failure session` lines in the log. This is not a `-N`-gated feature and not a compat_banner artifact; it's deliberate custom logic in `T2tHandler`.

### Location: `crates/tunnel2tunnel-ssh/src/lib.rs`

- **`auth_succeeded`** (lines 716–818): runs after every successful publickey auth, for *every* connecting principal, regardless of entity type (client vs server UUID) or whether the SSH client used `-N`.
  - Lines 721–727: fake-shell/tarpit path also opens a session channel.
  - Lines 759–808: the real path — `tokio::spawn` background task calls `handle.channel_open_session().await` (line 760) to push a "welcome" MOTD-style message (lines 770–776, `"✨ Welcome to tunnel2tunnel, {entity_name}! ✨ ... Twilight Sparkle has verified your access rules..."`) and then starts a periodic 5-minute "Derpy Hooves" keepalive ping loop (lines 778–800) over that same channel.
  - Line 805: if the channel open fails, it's only logged at `debug!` ("client may not support it") — no functional impact, confirming this is best-effort/decorative, not required for the tunnel to work.
  - `entity_type` (client vs server) only affects the *text* of the welcome string (line 774) — it does **not** gate whether the channel is opened. Both client-role and server-role principals get this pushed session channel identically.

- **`channel_open_session`** (lines 820–874): handles the reverse direction — if the *client* opens a session channel itself (plain interactive `ssh`, no `-N`), the server treats it as a chat/interactive console (comment at line 820: "Accept client-initiated session channels (interactive `ssh` without -N)"), sending the same welcome banner plus "Type a message and press Enter to chat with other connected entities."

- `lib.rs:43–61` — a `SessionEntry`/registry system supports "welcome, ping, broadcast, chat" across all connected sessions (comment at line 43), confirming this is a full interactive-console feature layered on top of the pure port-forwarding server.

### Why `ssh -N` shows "failure session"
With `-N`, OpenSSH's client never requests to open its own `session` channel (that's normal for `-N`). But per standard SSH semantics, only the client is supposed to open `session`-type channels; when the *server* opens one toward the client (as `auth_succeeded` does here, line 760), OpenSSH's client-side `client_input_channel_open` rejects it with `failure session`, exactly matching the log. This has no effect on the actual `-L` port forward, since that's handled via a separate `direct-tcpip`/`forwarded-tcpip` path (`channel_open_direct_tcpip`, line 940ff), unrelated to this session-channel push.

### Docs
No CLAUDE.md exists for this crate; `ai/plans/016_...md` investigates a *different*, unrelated local port-binding issue on the same host/command and does not mention this MOTD feature or gate it behind non-`-N` usage.