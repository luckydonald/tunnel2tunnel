In tunnel2tunnel repo, find:
1. Which backend Rust function/endpoint frontend/src/pages/EntityDetailPage.vue calls to fetch its connection logs table (look at frontend/src/api/entities.ts or admin.ts for a logs fetch call used by EntityDetailPage, and trace to the axum route in crates/tunnel2tunnel-web/src/routes/entities.rs, likely calling ConnectionLog::list_for_entity).
2. Full content of crates/tunnel2tunnel-web/src/routes/entities.rs around the ConnLogResponse struct (lines ~560-620) and wherever it's constructed from a ConnectionLog for the entity-logs endpoint.
3. Full content of crates/tunnel2tunnel-web/src/routes/tarpit.rs (whole file).
4. Full content of frontend/src/pages/AdminConnectionLogsPage.vue.
5. Full content of the relevant table section of frontend/src/pages/EntityDetailPage.vue (search "ConnLog" or logs table, roughly lines 600-700).
6. Full content of frontend/src/api/admin.ts (ConnLog interface, LogSearchParams/Result, searchConnectionLogs).

Report file contents/line ranges verbatim (quote the code) so I can edit directly without re-reading.