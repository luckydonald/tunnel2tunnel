# Fix: local SSH tunnel port conflict + reduce noise from server-pushed session channel

## Context

The user's `ssh -N -L 5902:...` tunnel command was failing to bind locally — root-caused (see "Investigation" below) to a stale duplicate `ssh` process already holding port 5902 on the client machine, not a tunnel2tunnel bug. That part is already fixed and verified working.

While debugging, a second, unrelated observation came up: every connection to the t2t SSH server shows a client-side `failure session` line in verbose output. Investigation of `crates/tunnel2tunnel-ssh/src/lib.rs` confirmed this is expected: `auth_succeeded` (lines 715–818) *unconditionally* spawns a task that opens a server-initiated `session` channel back to every authenticated client to push a welcome banner and a 5-minute "keepalive ping" loop (lines 759–808). Per SSH semantics only the client is supposed to open `session`-type channels, so when the SSH client used `-N` (as tunnel2tunnel's own frontend recommends, `SshCommandDisplay.vue:180`), OpenSSH legitimately rejects the server's push with `failure session` — cosmetic, no functional impact on the actual `-L`/`-R` forward (separate `direct-tcpip` channel), but it clutters `-v` logs and is pure waste (the channel is opened and immediately rejected on every `-N` connection).

The user wants:
1. **Server-side optimization**: skip opening that push channel entirely for connections that don't want it, instead of opening-then-being-rejected every time.
2. **Frontend**: since the currently-recommended command always uses `-N`, either drop `-N` from the default recommendation, or add a "non-interactive mode" checkbox (default matches today's behavior) that controls both the `-N` flag and this new server-side opt-out together, so the recommended command and the server's behavocor stay in sync and no longer produces the `failure session` noise for the default (non-interactive) case.

## Investigation findings (already confirmed by reading the code)

- `crates/tunnel2tunnel-ssh/src/lib.rs`
  - `auth_succeeded` (lines 715–818): the real (non-fake-shell) path unconditionally does `handle.channel_open_session()` (line 760) → welcome banner (770–776) → periodic ping loop (778–800), pushed from the **server** side to every client regardless of whether it wants it.
  - `channel_open_session` (lines 820–874): a **separate**, client-initiated handler that already fires whenever the SSH client itself opens a session channel — which plain `ssh` (no `-N`) does automatically as part of requesting an interactive shell, and which `-N` (by definition — "do not execute a remote command") never does. It already sends its own welcome banner (854–861) and registers the channel for chat, independent of the proactive push above.
  - Net effect today: for interactive (non-`-N`) clients, **both** handlers fire — the server's proactive push *and* the client's own session-open — producing a duplicated welcome message and two live channels/registrations for one connection. For `-N` clients, only the proactive push fires, and it's always rejected client-side (`failure session`), since only clients are supposed to initiate `session` channels.
  - **No new state, flag, or username convention is needed.** Simply deleting the proactive push and relying solely on the already-existing reactive `channel_open_session` path gives correct behavior for both cases for free: `-N` clients never open a session channel → server never tries → no noise, no chat (correct, since `-N` means non-interactive) → interactive clients open one exactly as they already do → existing handler fires exactly as it already does. This is a deletion/consolidation, not a new mechanism.
  - The only functionality to preserve: the periodic "Derpy Hooves" keepalive ping (currently lines 778–800, only wired to the proactive channel) needs to move into `channel_open_session` so interactive users keep getting it.
- `frontend/src/components/SshCommandDisplay.vue`
  - The command is built directly in the template (lines 180–198), not a single computed string. `-N` is a hardcoded literal (line 180).
  - Existing checkbox pattern to follow: `discovery-check` checkboxes (lines 126–131), plain `<input type="checkbox">` with `:checked` + `@change`; `EntityDetailPage.vue:456` shows the `label.checkbox-label` wrapper style used elsewhere in this codebase for checkboxes with adjacent text.
  - No entity/port-level setting exists yet for this in `frontend/src/api/entities.ts` — purely a display-time toggle (local `ref`), no backend API/DB changes needed for the frontend part.

## Plan

### Backend (`crates/tunnel2tunnel-ssh/src/lib.rs`)

1. In `auth_succeeded`, delete the entire proactive channel-open block (lines 751–808: the `handle.channel_open_session()` call, its welcome message, and the keepalive-ping spawn). Keep the registry insert (lines 739–749, still needed so `channel_open_session`/broadcast can find this connection) and the "broadcast arrival to other sessions" call (810–815) unchanged.
2. In `channel_open_session` (lines 820–874), after the existing welcome message (`handle.data(ch_id, welcome...)`, line 861), add the periodic keepalive-ping loop (moved verbatim from the deleted block, using this handler's `channel`/`handle`/`ch_id` instead) inside its existing background `tokio::spawn` (lines 863–871), replacing the current bare wait-loop with the same `tokio::select!` (ping-or-close) structure the old code used.
3. Result: only one place opens/sends a welcome+ping (the client-initiated handler), naturally gated on whether the client itself requested a session channel — which is exactly `-N` vs. non-`-N`.

### Frontend (`frontend/src/components/SshCommandDisplay.vue`)

1. Add a local `const nonInteractive = ref(false)` — **default off**, so the generated command omits `-N` and users see the MOTD/chat by default.
2. Add a checkbox near `cmd-header` (following the `discovery-check` pattern, wrapped in `label.checkbox-label`), labeled "Non-interactive mode" with helper text "Disables text channel providing status updates."
3. Make the `-N \` line in the template (line 180) conditional on `nonInteractive` — present when checked, omitted when not. No other part of the command (username, ports, host) needs to change, since the server-side fix above makes detection automatic.

### Out of scope / not needed
- No DB migration, no new API route, no username/protocol convention — this is a pure backend consolidation (delete dead/duplicate logic) plus a frontend display toggle.
- No change to the actual port-forwarding path (`channel_open_direct_tcpip`/`tcpip_forward`) — confirmed unaffected either way.

## Verification

1. **Backend**: `cargo build -p tunnel2tunnel-ssh` (or full workspace) compiles.
2. Connect with `ssh -N -i <key> <entity_id>@host -p 2222 -v` — verify no `channel_open_session`/`failure session` lines appear at all, and the `-L`/`-R` forward still works.
3. Connect with `ssh -i <key> <entity_id>@host -p 2222` (no `-N`, interactive) — verify exactly one welcome banner appears (not duplicated) and the periodic keepalive ping still arrives every 5 minutes.
4. **Frontend**: `cd frontend && npm run build` succeeds; visually check `EntityDetailPage` — checkbox unchecked by default shows the command without `-N`; checking it adds `-N` back. Copy-paste each generated command in a real terminal against the dev server to confirm both match the two backend cases above.
