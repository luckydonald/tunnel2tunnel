In this repo (tunnel2tunnel), find where SSH tunnel connection disconnect times get recorded and displayed. Look at:
- migrations/004_connection_logs.sql (schema for connection_logs table, likely has connected_at/disconnected_at columns)
- crates/tunnel2tunnel-ssh/ for where connection_logs rows get inserted/updated on channel close/disconnect
- crates/tunnel2tunnel-web/src/routes for the /api/entities/{id}/logs endpoint that reads connection_logs
- frontend/src for the component displaying logs (likely EntityDetail page) to see how disconnect time is rendered

Report: exact file:line where disconnect_at (or similar column) is set, whether it's actually updated when connection closes (not just left null), and where/how frontend renders it. Look for bugs: e.g. never updated, wrong duration calc, timezone issue, or update happening on wrong condition/event. Be concrete with code snippets and line numbers.