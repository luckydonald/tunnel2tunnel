In repo /home/user/git/luckydonald/tunnel2tunnel, explore crates/tunnel2tunnel-ssh in depth. I need a full understanding of:
1. The russh Handler implementation: auth_none, auth_password (if any), auth_publickey / auth_publickey_offered, auth_succeeded, channel_open_session, channel_open_direct_tcpip, and how connections/channels/state are tracked per-session.
2. Where and how it currently logs auth attempts (tracing::info! calls) — find exact log line locations and the format strings used (need exact field names: peer_ip, user, fp, entity_id, entity_name, conn_id, key_algo etc.)
3. Where fail2ban-format log writing happens currently (FAIL2BAN_LOG_PATH env var, any existing fail2ban writer module) — quote the code.
4. How entity lookup / key fingerprint lookup / entity_access checks work today (which functions, which crate).
5. How IP whitelist evaluation works (tunnel2tunnel-core/src/ip_whitelist.rs) — is there any existing rate-limiting or banning mechanism already, even partial?
6. Any existing "shell" or "channel_open_session" handling for authenticated users (to understand how a fake/bogus shell could be built) and how tokio::spawn is used for Handle confirmation calls per the CLAUDE.md gotcha.
7. Server struct / per-connection state struct definitions (fields) so I know what's available to extend (e.g. adding tarpit state).

Report file paths and line numbers for everything above, and quote the relevant code blocks (not just describe them). This is a research-only task, do not modify anything.