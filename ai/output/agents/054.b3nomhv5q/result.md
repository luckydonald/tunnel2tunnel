   Compiling t2t v0.1.0 (/home/user/git/luckydonald/tunnel2tunnel/crates/t2t)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.96s
     Running tests/tarpit_e2e.rs (target/debug/deps/tarpit_e2e-023f8ef6ba434b4d)

running 7 tests
test repeated_bans_eventually_engage_fake_shell ... ok
test repeated_bad_key_attempts_trigger_slow_auth_delay ... ok
test auth_none_probe_never_counts_toward_ban has been running for over 60 seconds
test ban_action_threshold_rejects_instantly_without_tarpit has been running for over 60 seconds
test legit_login_sequence_is_never_tarpitted has been running for over 60 seconds
test repeated_bans_eventually_engage_banner_drip has been running for over 60 seconds
test single_failed_login_marks_ended_at_on_disconnect has been running for over 60 seconds
test repeated_bans_eventually_engage_banner_drip ... ok
test ban_action_threshold_rejects_instantly_without_tarpit ... ok
test auth_none_probe_never_counts_toward_ban ... ok
test single_failed_login_marks_ended_at_on_disconnect ... ok
test legit_login_sequence_is_never_tarpitted ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 127.29s

