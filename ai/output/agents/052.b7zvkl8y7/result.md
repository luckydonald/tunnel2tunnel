   Compiling tunnel2tunnel-ssh v0.1.0 (/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-ssh)
   Compiling tunnel2tunnel-web v0.1.0 (/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web)
   Compiling t2t v0.1.0 (/home/user/git/luckydonald/tunnel2tunnel/crates/t2t)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.58s
     Running tests/tarpit_e2e.rs (target/debug/deps/tarpit_e2e-023f8ef6ba434b4d)

running 7 tests
test auth_none_probe_never_counts_toward_ban ... ok
test single_failed_login_marks_ended_at_on_disconnect ... ok
test repeated_bad_key_attempts_trigger_slow_auth_delay ... FAILED
test repeated_bans_eventually_engage_fake_shell ... FAILED
test repeated_bans_eventually_engage_banner_drip ... FAILED
test legit_login_sequence_is_never_tarpitted ... ok
test ban_action_threshold_rejects_instantly_without_tarpit ... ok

failures:

---- repeated_bad_key_attempts_trigger_slow_auth_delay stdout ----

thread 'repeated_bad_key_attempts_trigger_slow_auth_delay' (694626) panicked at crates/t2t/tests/tarpit_e2e.rs:167:10:
client connect: IO(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" })
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- repeated_bans_eventually_engage_fake_shell stdout ----

thread 'repeated_bans_eventually_engage_fake_shell' (694628) panicked at crates/t2t/tests/tarpit_e2e.rs:167:10:
client connect: IO(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" })

---- repeated_bans_eventually_engage_banner_drip stdout ----

thread 'repeated_bans_eventually_engage_banner_drip' (694627) panicked at crates/t2t/tests/tarpit_e2e.rs:167:10:
client connect: IO(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" })


failures:
    repeated_bad_key_attempts_trigger_slow_auth_delay
    repeated_bans_eventually_engage_banner_drip
    repeated_bans_eventually_engage_fake_shell

test result: FAILED. 4 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.49s

error: test failed, to rerun pass `-p t2t --test tarpit_e2e`
