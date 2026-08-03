Confirmed: only one `tracing::info!` call site produces "bridge established" in the whole codebase, and there is no retry/multiplex code around it (see analysis below) — the duplicate log line is a log-pipeline artifact, not a double-bridge bug.

## Handler location

`channel_open_direct_tcpip` — `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh/src/lib.rs:1064` (through `:1293`).

## Every return branch, with logging status

| Line(s) | Condition | Return | Logged? |
|---|---|---|---|
| 1073-1076 | not authenticated | `Ok(false)` | yes — `warn!` "not authenticated" |
| 1090-1099 | `resolve_target_entity` → no match (not a UUID/alias) | `Ok(false)` | yes — `info!` "target hostname not found" |
| 1102-1116 | target entity DB lookup `Ok(None)` | `Ok(false)` | yes — `info!` "target entity not found in db" |
| 1112-1115 | target entity DB lookup `Err(e)` | `Ok(false)` | yes — `error!` "db error loading target entity" |
| 1121-1143 | no enabled `port_config` for that proxy port | `Ok(false)` | yes — `warn!` "no enabled port_config" |
| 1139-1142 | DB error loading `port_config` | `Ok(false)` | yes — `error!` "db error loading port_config" |
| 1147-1169 | client has no `port_subscriptions` row | `Ok(false)` | yes — `warn!` "no subscription" |
| 1165-1168 | DB error loading subscription | `Ok(false)` | yes — `error!` "db error loading subscription" |
| 1170-1178 | subscription exists but `enabled == false` | `Ok(false)` | yes — `warn!` "subscription exists but is disabled" |
| 1183-1207 | cross-account, `EntityAccess::check_access` false **or errored** (`.unwrap_or(false)` swallows the `Err` at 1193, indistinguishable from a real denial) | `Ok(false)` | yes — `warn!` "access denied" (but a genuine DB error here is silently reclassified as "access denied" with no error-level log of the underlying DB failure) |
| 1214-1243 | server-liveness poll (12s/250ms) times out, no server_slot registered | `Ok(false)` | yes — `info!` "target server never came online within the retry window" (this is the PC6 path, logged twice in your session) |
| **1249-1252** | `server_handle.channel_open_forwarded_tcpip(...)` returns `Err(e)` | **`Err(anyhow!(...))` via `?`, propagated out of the function entirely** | **NO — completely silent, no `tracing::*!` call anywhere near this line** |
| 1292 | everything succeeds | `Ok(true)` | yes — `info!` "bridge established" (client_ch/server_ch) |

## The silent path

`crates/tunnel2tunnel-ssh/src/lib.rs:1249-1252`:

```rust
let server_ch = server_handle
    .channel_open_forwarded_tcpip(&registered_address, port_to_connect, "127.0.0.1", 0)
    .await
    .map_err(|e| anyhow::anyhow!("forwarded-tcpip open failed: {e:?}"))?;
```

This is the one and only branch in the function with zero `info!`/`warn!`/`error!` call. If the server-side session's `channel_open_forwarded_tcpip` RPC fails (e.g. the server-side SSH session that owns the `-R` forward has itself dropped/closed between the liveness poll succeeding and this call — a real TOCTOU window, since the poll at 1214-1233 only checks the slot map, not that the session is still alive), the error is converted to `anyhow::Error` and returned via `?` with no trace of *why* or *that* it happened.

However: I traced how russh's `server_handle_channel_open` (in `russh-0.61.2/src/server/encrypted.rs:1546-1621`) consumes this return value — `finalize_channel_open` (which is what actually calls `open.fail(..., SSH_OPEN_ADMINISTRATIVELY_PROHIBITED, b"Rejected")`, i.e. the exact string the client logged) is only invoked inside `if let Ok(allowed) = &mut result`. An `Err` from the handler skips that block entirely and the `Err` is propagated upward via `?` through `process_packet`, which most likely terminates the whole SSH connection/session rather than sending a clean per-channel `CHANNEL_OPEN_FAILURE`. So this silent `Err` path is real and worth fixing regardless, but it doesn't by itself explain a clean "channel 5: open failed: administratively prohibited: Rejected" on the client with the rest of the session continuing — that specific wire message can only originate from an `Ok(false)` branch (all of which are logged in this function) or from some other channel-open handler in the codebase.

That means either:
- there's a request for a *third* distinct host/channel not covered by your log excerpt that hit one of the logged `Ok(false)` branches but its log line got lost/filtered/rotated out of the log you're reading, or
- the rejection is for a channel type this function doesn't even see (e.g. `channel_open_session`, `channel_open_x11`, or `channel_open_direct_streamlocal` — check those handlers too, they're not in this file's excerpt if implemented elsewhere), or
- the access-check error-swallowing at line 1193 (`.unwrap_or(false)`) produced the generic "access denied" `warn!` you're not searching for by the right message text — this is the closest thing to a "logged but misleading" cause.

I'd recommend grepping the log for `client_ch=5` and `channel_open` broadly (not just "bridge established") to find whichever `warn!`/`info!` line actually fired for that specific channel-open failure, and separately fixing line 1249-1252 to log the error before/instead of the bare `?`.

## Channel reuse / multiplexing

There is no channel-multiplexing or reuse logic in this codebase for direct-tcpip — `self.bridges.insert(client_ch_id, ...)` (line 1257-1258) and `self.bridge_ids.insert(client_ch_id, bridge_id)` (line 1276) are both plain inserts, not checked for an existing key, and are only ever populated once per successful `channel_open_direct_tcpip` call and torn down in `channel_close` (lines 1345-1357) which removes both maps by `channel` id. If a client (e.g. a VNC viewer doing a connect retry) opened channel id 5 twice without the first `channel_close` having removed it from `self.bridges`, the second `insert` would silently overwrite the first entry (HashMap `insert` returns/discards the old value) — a real, latent double-bridge hazard, but it requires the SSH channel id to be reused before close, which OpenSSH's local id allocator would only do after a genuine `CHANNEL_CLOSE`. Given the identical-nanosecond-timestamp duplicate you observed, and that there's only one `tracing::info!("... bridge established")` call site in the whole repo, the far more likely explanation is a **duplicated log emission** (e.g. log file/journal double-write, `tee`/aggregator duplication, or the log viewer reading the same bytes twice) rather than the async handler actually executing twice — a genuine second execution would necessarily re-run the DB queries, the up-to-12s polling loop, and a fresh `channel_open_forwarded_tcpip` RPC, none of which can complete at literally the same nanosecond as the first run, and would very likely allocate a different `server_ch` id from russh's session-local channel counter rather than reusing `2`.