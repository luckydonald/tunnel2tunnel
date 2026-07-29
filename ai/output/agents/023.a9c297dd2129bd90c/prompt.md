In this Rust workspace (tunnel2tunnel, a self-hosted SSH rendezvous manager built on russh 0.61), I'm investigating an SSH client log where, after successful publickey auth on a `tcpip_forward`/local-forward connection made with `ssh -N -L 5902:...`, the SERVER appears to push a channel back to the CLIENT. The client-side verbose log shows:

```
debug1: Entering interactive session.
debug1: pledge: filesystem
debug1: client_input_channel_open: ctype session rchan 2 win 2097152 max 32768
debug1: failure session
```

This looks like the SSH server (crates/tunnel2tunnel-ssh) opened a channel of type "session" back toward the client after auth succeeded — which the OpenSSH client running with `-N` rejects (clients don't normally accept server-initiated "session" channels; only clients open those toward servers in standard SSH).

Please investigate crates/tunnel2tunnel-ssh/src/ and find:
1. Is there any code path where the server calls something like `channel_open_session` or similar on the client's `Handle` after `auth_succeeded`, e.g. to present a MOTD or an "interactive chat"/management console to entities connecting?
2. Search for anything related to "MOTD", "welcome message", "interactive", "chat", banner text, or a session channel opened by server logic (not just the tcpip_forward/direct-tcpip proxy paths).
3. Look at the `Handler` trait impl (auth_succeeded, channel_open_forwarded_tcpip, channel_open_direct_tcpip, tcpip_forward, etc.) — is there logic that behaves differently based on whether the connecting principal is a "client" entity type vs a "server" entity type? E.g. does connecting AS a server-role entity (the `019f15ec-...` UUID in the command, used as SSH username) trigger a different behavior than a pure port-forward client?
4. Check if there's a CLAUDE.md or docs mention of this MOTD/interactive-session feature, and whether it's gated behind NOT using `-N`, or whether it's unconditional and thus incompatible with `ssh -N`.

Report: file paths and line numbers for anything relevant, and a clear explanation of whether the server unconditionally tries to open a session/MOTD channel back to every authenticated connection (which would explain the "failure session" line appearing even during a pure `-N` forward-only connection), or whether this is something else entirely (e.g. a compat_banner/keepalive artifact unrelated to any custom session logic). Keep the report under 400 words.