# Fix: local SSH tunnel port conflict + reduce noise from server-pushed session channel

## Context

The user's `ssh -N -L 5902:...` tunnel command was failing to bind locally — root-caused (see "Investigation" below) to a stale duplicate `ssh` process already holding port 5902 on the client machine, not a tunnel2tunnel bug. That part is already fixed and verified working.

While debugging, a second, unrelated observation came up: every connection to the t2t SSH server shows a client-side `failure session` line in verbose output. Investigation of `crates/tunnel2tunnel-ssh/src/lib.rs` confirmed this is expected: `auth_succeeded` (lines 715–818) *unconditionally* spawns a task that opens a server-initiated `session` channel back to every authenticated client to push a welcome banner and a 5-minute "keepalive ping" loop (lines 759–808). Per SSH semantics only the client is supposed to open `session`-type channels, so when the SSH client used `-N` (as tunnel2tunnel's own frontend recommends, `SshCommandDisplay.vue:180`), OpenSSH legitimately rejects the server's push with `failure session` — cosmetic, no functional impact on the actual `-L`/`-R` forward (separate `direct-tcpip` channel), but it clutters `-v` logs and is pure waste (the channel is opened and immediately rejected on every `-N` connection).

The user wants:
1. **Server-side optimization**: skip opening that push channel entirely for connections that don't want it, instead of opening-then-being-rejected every time.
2. **Frontend**: since the currently-recommended command always uses `-N`, either drop `-N` from the default recommendation, or add a "non-interactive mode" checkbox (default matches today's behavior) that controls both the `-N` flag and this new server-side opt-out together, so the recommended command and the server's behavocor stay in sync and no longer produces the `failure session` noise for the default (non-interactive) case.

## Investigation findings (already confirmed by reading the code)

- `crates/tunnel2tunnel-ssh/src/lib.rs`
  - `auth_publickey` (lines 494–713): authenticates purely by public-key fingerprint (`SshKey::find_by_fingerprint`); the SSH `user` string (the username in `<uuid>@host`) is **not** validated against the entity id — it's logged only (`log_auth_success(&entity, &fp, user)` at line 709). This makes it safe to append a recognized suffix to `user` without breaking auth.
  - `auth_succeeded` (lines 715–818): the real (non-fake-shell) path unconditionally does `handle.channel_open_session()` (line 760) → welcome banner (770–776) → periodic ping loop (778–800). No existing flag gates this.
  - `T2tHandler` struct (lines 226–251): per-connection state — add a new field here, e.g. `quiet_session: bool`.
  - No `env_request`/`SetEnv` handler exists anywhere in the file (confirmed via grep) — so an SSH env var is not a ready-made hook; a username suffix is the lowest-effort mechanism given the auth code above.
- `frontend/src/components/SshCommandDisplay.vue`
  - The command is built directly in the template (lines 180–198), not a single computed string. `-N` is a hardcoded literal (line 180). The username interpolation is `{{ entity.id }}@{{ t2tHost }}` (line 197).
  - Existing checkbox pattern to follow: `discovery-check` checkboxes (lines 126–131), plain `<input type="checkbox">` with `:checked` + `@change` emitting an event to the parent (this component holds no local persisted state today; `EntityDetailPage.vue:456` shows the `label.checkbox-label` wrapper style used elsewhere in this codebase for checkboxes with adjacent text).
  - No entity/port-level "non-interactive" setting exists yet in `frontend/src/api/entities.ts` types — this is purely a display-time toggle, not persisted per entity, so a local `ref` in `SshCommandDisplay.vue` (or a lifted prop from `EntityDetailPage.vue` if it should persist across visits) is sufficient — no backend API/DB changes needed for the frontend part.

## Plan

### Backend (`crates/tunnel2tunnel-ssh/src/lib.rs`)

1. Add `quiet_session: bool` to `T2tHandler` (near `fake_shell`, line ~250) and initialize `false` at construction (line ~211).
2. In `auth_publickey`, before doing anything else with `user`: check for a recognized suffix, e.g. `user.strip_suffix("+quiet")`. If present, set `self.quiet_session = true` and use the stripped string for all subsequent logging/handling in that function (fingerprint lookup is keyed off the public key, not `user`, so stripping doesn't affect auth — only cleans up what gets logged).
3. In `auth_succeeded`, wrap the channel-open block (lines 751–808) in `if !self.quiet_session { ... }` — when quiet, skip opening the push channel (and its registry entry / welcome / keepalive spawn) entirely. Leave the "broadcast arrival to other sessions" call (lines 810–815) unchanged — other connected entities should still see the join notice; only *this* session opts out of receiving pushes.
4. No change needed to `channel_open_session` (the reverse, client-initiated chat path) — a client using `-N` never triggers it anyway, and non-`-N` interactive users keep the existing chat feature untouched.

### Frontend (`frontend/src/components/SshCommandDisplay.vue`)

1. Add a local `const nonInteractive = ref(true)` (default `true` to match today's `-N`-only behavior, so existing users see no change unless they opt out).
2. Add a checkbox near the `cmd-header` (following the `discovery-check` pattern, wrapped in a `label.checkbox-label` per the codebase's existing convention), labeled "Non-interactive mode" with helper text "Disables text channel providing status updates."
3. Make the command template conditional on `nonInteractive`:
   - When `true` (current behavior): keep `-N \` and use `{{ entity.id }}+quiet@{{ t2tHost }}` as the username.
   - When `false`: drop the `-N \` line entirely and use the plain `{{ entity.id }}@{{ t2tHost }}` username — this is what lets the user see the MOTD/chat, since dropping `-N` lets the OpenSSH client actually open its own session channel (handled server-side by the pre-existing `channel_open_session`).
4. No new prop/API call required — purely a template/local-state change in this one component.

### Out of scope / not needed
- No DB migration, no new API route — this is a display-time toggle plus a client-visible username convention, not a persisted setting.
- No change to the actual port-forwarding path (`channel_open_direct_tcpip`/`tcpip_forward`) — confirmed unaffected either way.

## Verification

1. **Backend**: `cargo build -p tunnel2tunnel-ssh` (or full workspace) compiles. Manually connect with `ssh -N -i <key> <entity_id>+quiet@host -p 2222 -v` — verify no `channel_open_session`/`failure session` lines appear at all (server never opens the channel), while a forward still works. Then connect with `ssh -N -i <key> <entity_id>@host -p 2222 -v` (no suffix) — verify the existing `failure session` line still appears (unchanged default behavior for old/updated clients not using the new suffix).
2. **Backend interactive**: `ssh -i <key> <entity_id>@host -p 2222` (no `-N`) still shows the welcome banner and chat prompt as before.
3. **Frontend**: `cd frontend && npm run build` succeeds; visually check `EntityDetailPage` — checkbox toggles the displayed command between the `-N ... +quiet@...` form and the plain interactive form, matching the two backend cases tested above. Copy-paste each generated command in a real terminal against the dev server to confirm both work end-to-end.
