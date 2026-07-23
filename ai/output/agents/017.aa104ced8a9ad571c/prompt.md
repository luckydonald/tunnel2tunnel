In tunnel2tunnel repo, I need full detail on the tarpit/ban threshold engine and its admin UI, because we're redesigning the global tarpit threshold setting (currently one count+window pair in the `settings` kv table) into a table of multiple threshold rules (e.g. "5 fails in 10 min" AND/OR "20 fails in 24h" AND "50 fails in a week" simultaneously).

Report, with exact file paths, line numbers, and quoted code:

1. Full content of `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs` — especially `TarpitEntry` struct, `TarpitState` type, `record_failure()`, whatever function checks the threshold_count/threshold_window_seconds settings and decides to trigger a ban, and how `trigger_count`/`fail_count` are used. Also find where `Settings::get(pool, "tarpit_threshold_count")` / `"tarpit_threshold_window_seconds")` / `"tarpit_enabled"` are read (grep for these key strings across `crates/`).
2. Where `count_recent_failures_for_peer_ip` / `count_recent_failures_for_user` (in `crates/tunnel2tunnel-core/src/models/connection_log.rs`) are actually called from — i.e. is the ban-trigger decision DB-driven per attempt, or purely from the in-memory `TarpitEntry` counters? Quote the call site(s).
3. The admin frontend page/component that currently renders the tarpit settings form (calls `adminApi.getTarpitSettings()` / `updateTarpitSettings()`) — likely `frontend/src/pages/Settings.vue` or similar. Quote the relevant script + template section (the form for threshold_count/threshold_window_seconds/enabled).
4. The `BanRule` Rust model (`crates/tunnel2tunnel-core/src/models/ban_rule.rs`) in full — I want to reuse its CRUD pattern (list_all/create/delete) as a template for a new `tarpit_threshold` table+model.
5. Any existing migration file structure/naming convention to follow for numbering the next migration (check `migrations/` directory listing, confirm 008 is the latest, or if there's a 009+ already).

This is investigation only — do not write any code or plans.