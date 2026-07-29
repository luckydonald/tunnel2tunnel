# Fix: local SSH tunnel command failing on port 5902

## Update (follow-up run)

After the fix below was applied, the tunnel (PID `3013778`) came up successfully and is still listening on `127.0.0.1:5902` / `[::1]:5902`. The user then manually ran the exact same `ssh -N -L 5902:...` command again (task `bgdl8e31t`, PID `3015244`), which failed with the identical "Address already in use" — expected, since it's a redundant second copy of the already-working tunnel, not a new bug.

## Update 2: "failure session" / no MOTD explained

The user expected to see a MOTD/interactive chat over this connection. Investigated `crates/tunnel2tunnel-ssh/src/lib.rs`:

- `auth_succeeded` (lines 716–818) unconditionally `tokio::spawn`s a task that calls `handle.channel_open_session()` on **every** successful auth (any entity type, any client flags) to push a welcome banner ("✨ Welcome to tunnel2tunnel...") and a periodic keepalive ping. If that channel open fails, it's only `debug!`-logged (line 805) — best-effort, no functional impact.
- Per standard SSH semantics only the *client* is supposed to open `session`-type channels; a server-initiated one is what OpenSSH's `client_input_channel_open` is rejecting with `failure session`. This is harmless noise and does not affect the actual `-L` port forward, which rides a separate `direct-tcpip`/`forwarded-tcpip` channel (`channel_open_direct_tcpip`, ~line 940).
- `channel_open_session` (line 820) is the *reverse* path: if the SSH **client** opens its own session channel (i.e. plain interactive `ssh`, no `-N`), the server treats that as the interactive chat console and sends the same welcome text plus "Type a message and press Enter to chat with other connected entities."
- Net effect: `-N` gets you the port forward silently (working as intended) but will never show the MOTD/chat, because `-N` suppresses the client-side session-channel request that the chat feature is built on. To see the MOTD/chat, a separate plain `ssh <same target> -p 2222` (without `-N`, no `-L`) is needed — it won't establish a port forward, just the interactive console.

No code change is required for the original ask (tunnel works). This section is purely explaining observed behavior; only add follow-up work here if the user asks for a code/behavior change (e.g. suppressing the "failure session" noise, or documenting it).


## Context

The user ran:
```
ssh -N -i ~/.ssh/t2t_fedora_work -L 5902:019f1899-9af7-7f30-aeb1-8bb9de8fd347:5900 019f15ec-fb3b-7f41-ba4b-84a0baa6b3cd@tunnel2tunnel-d5oteit4omkq577f6j4r3rd9.h1.bn-x.de -p 2222 -v
```
in the background to open a local port-forward through the tunnel2tunnel SSH rendezvous server (VNC-style tunnel to a remote entity on port 5900). The verbose log shows the connection actually succeeded end-to-end — TCP connect, key exchange, host key verification, and publickey auth to the t2t SSH server (russh) all completed fine. This is **not** a tunnel2tunnel bug.

The failure is purely local:
```
bind [::1]:5902: Address already in use
bind [127.0.0.1]:5902: Address already in use
channel_setup_fwd_listener_tcpip: cannot listen to port: 5902
Could not request local forwarding.
```

`ss -ltnp` / `lsof -i :5902` / `ps -fp` confirm an **earlier** ssh process, PID `2925881`, launched from the same terminal (`pts/7`) at 18:37 running the identical command, is already bound to local port 5902. The new backgrounded process (PID `2927842`, wrapped by bash PID `2927402`) then failed to grab the same port and the local forward never came up, even though the remote session is alive and idling ("Entering interactive session").

The user chose: kill the stale process holding port 5902.

## Plan

1. Kill PID `2925881` (the older, redundant `ssh -L 5902:...` process from `pts/7`) with `kill 2925881` (plain SIGTERM, not `-9`, so it closes the tunnel cleanly).
2. Confirm port 5902 is free: `ss -ltnp | grep 5902` should return nothing for that PID.
3. The currently-backgrounded new tunnel process (PID `2927842`, task ID `bqy7u01ly`) will not automatically retry the bind — it already emitted "Could not request local forwarding" and moved on into the interactive session without a working forward. Stop it (`TaskStop`/kill PID `2927842` or its wrapper `2927402`) and re-run the original `ssh -N -i ~/.ssh/t2t_fedora_work -L 5902:...` command in the background again so it can bind cleanly now that 5902 is free.
4. Verify: `ss -ltnp | grep 5902` shows the new ssh process listening, and a local VNC client connecting to `localhost:5902` reaches the remote `5900` service through the tunnel.

## Verification

- `ss -ltnp | grep 5902` shows exactly one ssh process bound, matching the newly (re)started tunnel's PID.
- No `Address already in use` in the new command's verbose (`-v`) output.
- Optional: `nc -zv localhost 5902` succeeds, or open a VNC client pointed at `localhost:5902`.
