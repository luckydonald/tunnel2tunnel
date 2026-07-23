In this Rust+Vue repo (tunnel2tunnel), I need to add a "strike number" column to the connection log admin view. Find:

1. The `connection_logs` table schema (migrations/004_connection_logs.sql) and any related migrations mentioning "strike" or "tarpit" or "ban".
2. The Rust model/struct for connection logs (likely in tunnel2tunnel-core or tunnel2tunnel-ssh), especially any field related to strikes/failed attempts/ban count.
3. The tarpit/ban-rules feature implementation in tunnel2tunnel-ssh (search for "tarpit", "strike", "ban" in crates/).
4. The web API route serving connection logs (crates/tunnel2tunnel-web/src/routes, likely something like logs.rs).
5. The frontend admin connection logs page/component (frontend/src/pages, look for AdminConnectionLogs or similar) and its table columns definition, plus frontend/src/api/admin.ts types for connection logs.

Report exact file paths, line numbers, and relevant struct/type definitions, especially anything named "strike" already existing (count, number, index) so I can see if the data already exists and just needs to be displayed, or if it needs to be computed.