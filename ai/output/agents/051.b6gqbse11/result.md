    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.23s
     Running unittests src/main.rs (target/debug/deps/t2t-e443e63f0672fda1)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/tarpit_e2e.rs (target/debug/deps/tarpit_e2e-023f8ef6ba434b4d)

running 7 tests
test single_failed_login_marks_ended_at_on_disconnect ... ok
test repeated_bans_eventually_engage_banner_drip ... FAILED
test ban_action_threshold_rejects_instantly_without_tarpit ... FAILED
test repeated_bad_key_attempts_trigger_slow_auth_delay ... FAILED
test repeated_bans_eventually_engage_fake_shell ... FAILED
test auth_none_probe_never_counts_toward_ban ... ok
test legit_login_sequence_is_never_tarpitted ... ok

failures:

---- repeated_bans_eventually_engage_banner_drip stdout ----

thread 'repeated_bans_eventually_engage_banner_drip' (665652) panicked at crates/t2t/tests/tarpit_e2e.rs:441:5:
banner-drip must never send the real SSH-2.0 identification line, got: "SSH-2.0-russh_0.61.2\r\n"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- ban_action_threshold_rejects_instantly_without_tarpit stdout ----

thread 'ban_action_threshold_rejects_instantly_without_tarpit' (665649) panicked at crates/t2t/tests/tarpit_e2e.rs:561:22:
hard ban must never send any data (no tarpit method engages), got 22 bytes: [83, 83, 72, 45, 50, 46, 48, 45, 114, 117, 115, 115, 104, 95, 48, 46, 54, 49, 46, 50, 13, 10]

---- repeated_bad_key_attempts_trigger_slow_auth_delay stdout ----

thread 'repeated_bad_key_attempts_trigger_slow_auth_delay' (665651) panicked at crates/t2t/tests/tarpit_e2e.rs:369:5:
6th attempt should have been slow-auth-delayed after crossing the ban threshold, took 1.062581229s

---- repeated_bans_eventually_engage_fake_shell stdout ----

thread 'repeated_bans_eventually_engage_fake_shell' (665653) panicked at crates/t2t/tests/tarpit_e2e.rs:489:5:
an unregistered key on the fake-shell round-robin slot must still get Auth::Accept


failures:
    ban_action_threshold_rejects_instantly_without_tarpit
    repeated_bad_key_attempts_trigger_slow_auth_delay
    repeated_bans_eventually_engage_banner_drip
    repeated_bans_eventually_engage_fake_shell

test result: FAILED. 3 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 50.89s

error: test failed, to rerun pass `-p t2t --test tarpit_e2e`
