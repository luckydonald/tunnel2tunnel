# Fix premature disconnect on client-initiated SSH session channel

## Context

`RustConn` connects with a plain `ssh -L 5902:...:5900 ...` (no `-N`), so besides the
`-L` port forward it also opens a normal interactive session channel (visible in the
client log as `channel 2: new session [client-session]` via the ControlMaster mux).
The server (`crates/tunnel2tunnel-ssh/src/lib.rs`) accepts that channel in
`channel_open_session` (`lib.rs:765-832`), sends the "Welcome to tunnel2tunnel" banner,
and spawns a keep-alive task — but 6-7 seconds later the server closes the whole
connection ("closed by remote host" client-side / "connection closed (authenticated)"
server-side), even though nothing in `T2tHandler` (no timer, no idle-GC, no duplicate-
session eviction) explicitly closes it, and russh's own defaults
(`inactivity_timeout: 600s`, `keepalive_interval: None`) don't explain a 6-7s gap either.

Investigation (see below) narrowed it to a spec gap: after opening the session channel,
an interactive (non-`-N`) OpenSSH client always follows up with channel requests —
`pty-req`, `env`, `shell` (each sent with `want_reply = true`). `T2tHandler` implements
none of `pty_request` / `env_request` / `shell_request` / `exec_request` /
`window_change_request` / `subsystem_request`, so russh's default trait impls run
(`async { Ok(()) }`, confirmed in `russh-0.61.2/src/server/mod.rs:504-733`) — they never
call `session.channel_success(...)` or `channel_failure(...)`. The client is left
waiting forever for a reply to a `want_reply` request it's entitled to get answered.
This is the most likely trigger for the client eventually giving up and the connection
being torn down; it is also a plain SSH protocol violation independent of whether it's
the exact root cause, so it should be fixed regardless.

The exact code path that terminates the connection could not be pinned down further from
the existing `RUST_LOG=info` logs alone, because the accept loop in `lib.rs` currently
discards the result of the session future:

```rust
// lib.rs:167-174 (current)
tokio::spawn(async move {
    match russh::server::run_stream(cfg, socket, handler).await {
        Ok(session) => {
            let _ = session.await;   // any Err here is silently dropped
        }
        Err(e) => tracing::debug!(err = %e, "SSH: connection setup failed"),
    }
});
```

So the plan pairs the protocol fix with instrumentation, in case the ack fix alone
doesn't fully resolve it and a follow-up round of logs is needed.

## Changes

**File: `crates/tunnel2tunnel-ssh/src/lib.rs`**

1. Log the discarded `session.await` error instead of swallowing it:
   ```rust
   Ok(session) => {
       if let Err(e) = session.await {
           tracing::warn!(err = %e, "SSH: session ended with error");
       }
   }
   ```

2. Implement the missing channel-request handlers on `impl Handler for T2tHandler`,
   right after `channel_open_session` (~line 832), each just acknowledging success since
   this server doesn't provide a real shell/pty — only the welcome/chat channel and
   tunnel bridging matter:
   - `pty_request` — call `session.channel_success(channel)?;` ignore term/size args.
   - `env_request` — call `session.channel_success(channel)?;` ignore the variable.
   - `shell_request` — call `session.channel_success(channel)?;` (this is what turns the
     client's local terminal into "attached" mode — matches the existing welcome/chat
     UX, since chat input already flows through `data()` on this same channel).
   - `exec_request` / `subsystem_request` / `window_change_request` — reply with
     `session.channel_failure(channel)?;` (or, for `window_change_request`, a no-op
     `channel_success` since it carries no semantic meaning here) since this server has
     no exec/subsystem support — the important thing is that every `want_reply` request
     gets *some* reply instead of none.

   Follow the exact signatures/doc examples already given by russh in
   `russh-0.61.2/src/server/mod.rs:483-733` (the crate's own doc comments show the
   `session.channel_success(channel); Ok(())` pattern to copy).

## Verification

1. `cargo build -p tunnel2tunnel-ssh` (and `cargo build -p t2t`) to confirm the trait
   impls compile against russh 0.61's `Handler` signatures.
2. Run the stack locally per the `CLAUDE.md` "Running locally" steps, register a test
   entity + key, then reproduce with a plain (non `-N`) `ssh -L <local>:<target>:<port> ...`
   client command against `localhost:2222`, watching:
   - client-side `-v` log no longer shows an unexplained "closed by remote host" a few
     seconds after "Entering interactive session"
   - server log (`RUST_LOG=info`, or `tunnel2tunnel_ssh=debug` for the new
     `channel_open_session` debug line) shows no `"SSH: session ended with error"` warning
   - the welcome banner + chat still work as before (type a line, confirm broadcast to
     another connected session)
3. If the disconnect still reproduces after the ack fix, the new `session.await` error
   log will show the actual russh-level error on the next occurrence — use that to
   determine the real next step rather than guessing further.
