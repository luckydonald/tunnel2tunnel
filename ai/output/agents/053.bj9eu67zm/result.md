    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
     Running tests/tarpit_e2e.rs (target/debug/deps/tarpit_e2e-023f8ef6ba434b4d)

running 7 tests
test repeated_bans_eventually_engage_fake_shell ... ok
test legit_login_sequence_is_never_tarpitted ... ok
test auth_none_probe_never_counts_toward_ban has been running for over 60 seconds
test ban_action_threshold_rejects_instantly_without_tarpit has been running for over 60 seconds
test repeated_bad_key_attempts_trigger_slow_auth_delay has been running for over 60 seconds
test repeated_bans_eventually_engage_banner_drip has been running for over 60 seconds
test single_failed_login_marks_ended_at_on_disconnect has been running for over 60 seconds
test repeated_bans_eventually_engage_banner_drip ... ok
test single_failed_login_marks_ended_at_on_disconnect ... ok
test auth_none_probe_never_counts_toward_ban ... ok
test ban_action_threshold_rejects_instantly_without_tarpit ... ok
test repeated_bad_key_attempts_trigger_slow_auth_delay ... FAILED

failures:

---- repeated_bad_key_attempts_trigger_slow_auth_delay stdout ----

thread 'repeated_bad_key_attempts_trigger_slow_auth_delay' (705365) panicked at crates/t2t/tests/tarpit_e2e.rs:362:13:
attempt 4 should not yet be tarpitted, took 3.36789186s
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    repeated_bad_key_attempts_trigger_slow_auth_delay

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 114.92s

error: test failed, to rerun pass `-p t2t --test tarpit_e2e`
