# Close stale open connection_logs rows on boot

## Context

`connection_logs.ended_at` is set to `NOW()` via `ConnectionLog::set_ended()` when an SSH
session ends normally (`tunnel2tunnel-ssh/src/lib.rs:1181`, and the tarpit paths in
`tarpit/mod.rs` / `tarpit/banner_drip.rs`). When Coolify redeploys the app, the process is
killed/restarted without a graceful shutdown hook, so any connections that were open at that
moment never get their `ended_at` written — they're stuck with `ended_at IS NULL` forever.

This matters because `ConnectionLog::entity_status()` / `entity_statuses()`
(`tunnel2tunnel-core/src/models/connection_log.rs:319-358`) derive "online/offline" purely from
`bool_or(ended_at IS NULL)` on successful rows. A leaked open row makes an entity show as
permanently "online" in the dashboard even though the process (and the actual SSH connection)
is long gone.

Fix: on every boot, after migrations run and before the SSH/HTTP servers start accepting
traffic, mark every currently-open `connection_logs` row (`ended_at IS NULL`) as ended. Any
session that was genuinely still alive from before the restart is dead anyway (the TCP
connection was severed when the process died), so this is safe and correct in all cases, not
just a stale-cleanup heuristic.

## Implementation

1. **`crates/tunnel2tunnel-core/src/models/connection_log.rs`**
   Add a new associated function, alongside `set_ended`:

   ```rust
   /// Closes every still-open row (`ended_at IS NULL`) at process boot. Any
   /// connection that was open before this process started is necessarily
   /// dead — the TCP connection did not survive the restart — so this is not
   /// a heuristic, it's a correction of rows a prior process couldn't close
   /// during an ungraceful shutdown (e.g. a Coolify redeploy).
   pub async fn close_all_open_on_boot(pool: &PgPool) -> Result<u64, CoreError> {
       let result = sqlx::query("UPDATE connection_logs SET ended_at = NOW() WHERE ended_at IS NULL")
           .execute(pool)
           .await
           .map_err(CoreError::Sqlx)?;
       Ok(result.rows_affected())
   }
   ```

2. **`crates/t2t/src/main.rs`**
   In `run()`, right after the migrations block (`main.rs:53-58`) and before
   `bootstrap_admin`/server startup, call it and log the count:

   ```rust
   let closed = tunnel2tunnel_core::models::connection_log::ConnectionLog::close_all_open_on_boot(&pool)
       .await
       .context("failed to close stale open connection_logs rows on boot")?;
   if closed > 0 {
       tracing::info!(closed, "closed stale open connection_logs rows from a previous run");
   }
   ```

   Add the import at the top alongside the other `tunnel2tunnel_core`/`tunnel2tunnel_ssh` uses,
   or reference it fully qualified as above — match whatever's more idiomatic given the existing
   `use` block (currently only `db` is imported from `tunnel2tunnel_core`, so add
   `models::connection_log::ConnectionLog` to that).

   Placement must be *after* `sqlx::migrate!(...).run(&pool)` (table must exist / be current
   schema) and *before* `start_http`/`start_ssh` spawn (so no new rows can be created and
   incorrectly swept up in the same query — though since this runs once, synchronously, before
   either server task is spawned, that race can't happen anyway).

## Verification

- `cargo build -p t2t -p tunnel2tunnel-core` to confirm it compiles.
- Add a unit/integration test in `crates/tunnel2tunnel-core/tests/tarpit_models.rs` (which
  already has the `test_pool()` helper and exercises `ConnectionLog::create` /
  `entity_status`): create a successful `ConnectionLog` row with no `ended_at`, call
  `close_all_open_on_boot`, assert the row's `ended_at` is now `Some(...)`, and assert a row
  that already had `ended_at` set is left untouched (its `ended_at` timestamp doesn't change).
- Manually: start the app, open an SSH tunnel so a `connection_logs` row has `ended_at IS NULL`
  and the entity shows "online" in the dashboard, kill the process with `kill -9` (simulating
  Coolify's hard restart), restart it, and confirm the entity now shows "offline" and the log
  row has `ended_at` populated.
