In the Rust workspace at /Users/user/Documents/programming/Rust/tunnel2tunnel, investigate the crate `crates/tunnel2tunnel-ssh` (a russh 0.61-based SSH server). 

Context: A user is trying to SSH-tunnel in with `ssh -N -i <key> -R 5900:localhost:5900 <uuid>@<host> -p 2222`. Verbose client logs show: the server correctly performs key exchange, accepts the SSH connection, and responds with `SSH_MSG_USERAUTH_PK_OK` when the client probes with the public key (meaning fingerprint lookup succeeded). But when the client sends the actual signed publickey auth request afterward, the connection just hangs — no accept, no reject, nothing. This suggests a bug or deadlock/stuck-await in the server's `auth_publickey` handler AFTER signature verification succeeds, somewhere in the flow described in CLAUDE.md as: "Auth flow: fingerprint lookup → key/entity expiry → IP whitelist → entity_access check → admit."

Please find and report back (with file paths and line numbers):
1. The full implementation of the russh `Handler` trait's `auth_publickey` (and `auth_publickey_offered` if separate) methods — show the complete code.
2. Any locks (Mutex/RwLock) acquired in that path, and whether any of them could deadlock (e.g., a lock already held elsewhere, or a lock acquired twice) or whether an await on a lock could hang forever.
3. Any database queries in that path — could a query hang (e.g., waiting on a connection pool that's exhausted, or a query against a missing/locked table)?
4. Any explicit `Auth::Accept` / `Auth::Reject` return paths — check that ALL code paths through this function actually return a value (no branch that falls through without returning, no `.await` on something that never resolves).
5. Look at recent git history (`git log --oneline -30 -- crates/tunnel2tunnel-ssh`) for any recent changes to this auth path that might have introduced a regression.
6. Check the server slot map `Arc<Mutex<HashMap<(Uuid, u32), Handle>>>` mentioned in CLAUDE.md — is it touched during auth, and could it be involved in a hang?

Report full code snippets, not just summaries, so we can review the actual logic. Keep total response under 500 lines.