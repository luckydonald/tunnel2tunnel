…
exclusive ... ok
test search_filters_tarpit_presence_actions_and_timestamps ... FAILED
test search_filters_by_success_and_peer_ip ... FAILED
test count_recent_failures_respects_window ... ok
test search_paginates_and_filters_by_tarpit_method ... FAILED
test ban_rule_list_active_excludes_expired ... ok
test ban_rule_scope_check_enforced_both_directions ... ok
test close_all_open_on_boot_closes_only_open_rows ... ok
test entity_statuses_batches_multiple_entities ... ok
test entity_status_reflects_open_and_closed_sessions ... ok

failures:

---- search_filters_tarpit_presence_actions_and_timestamps stdout ----

thread 'search_filters_tarpit_presence_actions_and_timestamps' (724378) panicked at crates/tunnel2tunnel-core/tests/tarpit_models.rs:435:6:
called `Result::unwrap()` on an `Err` value: Sqlx(Database(PgDatabaseError { severity: Error, code: "23514", message: "new row for relation \"connection_logs\" violates check constraint \"connection_logs_fail_reason_check\"", detail: Some("Failing row contains (019fd0ac-35cb-7300-bff7-e2bcc8cdb5b0, null, 203.0.113.238, null, banned, null, null, 2026-08-05 06:46:14.694115+00, null, 2026-08-05 06:46:14.732138+00, 2026-08-05 06:46:14.732138+00, null, f, null, null, null, null, ban, null, null)."), hint: None, position: None, where: None, schema: Some("public"), table: Some("connection_logs"), column: None, data_type: None, constraint: Some("connection_logs_fail_reason_check"), file: Some("execMain.c"), line: Some(2076), routine: Some("ExecConstraints") }))
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- search_filters_by_success_and_peer_ip stdout ----

thread 'search_filters_by_success_and_peer_ip' (724377) panicked at crates/tunnel2tunnel-core/tests/tarpit_models.rs:263:6:
search: Sqlx(Database(PgDatabaseError { severity: Error, code: "42601", message: "syntax error at or near \"AND\"", detail: None, hint: None, position: Some(Original(57)), where: None, schema: None, table: None, column: None, data_type: None, constraint: None, file: Some("scan.l"), line: Some(1236), routine: Some("scanner_yyerror") }))

---- search_paginates_and_filters_by_tarpit_method stdout ----

thread 'search_paginates_and_filters_by_tarpit_method' (724379) panicked at crates/tunnel2tunnel-core/tests/tarpit_models.rs:333:6:
search page 1: Sqlx(Database(PgDatabaseError { severity: Error, code: "42601", message: "syntax error at or near \"AND\"", detail: None, hint: None, position: Some(Original(57)), where: None, schema: None, table: None, column: None, data_type: None, constraint: None, file: Some("scan.l"), line: Some(1236), routine: Some("scanner_yyerror") }))


failures:
    search_filters_by_success_and_peer_ip
    search_filters_tarpit_presence_actions_and_timestamps
    search_paginates_and_filters_by_tarpit_method

test result: FAILED. 8 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.44s

error: test failed, to rerun pass `-p tunnel2tunnel-core --test tarpit_models`
