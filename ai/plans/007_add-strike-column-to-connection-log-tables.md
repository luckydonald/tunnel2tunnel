# Add "strike #" column to connection log tables

## Context

Admin connection-log views (`AdminConnectionLogsPage.vue` global search, `EntityDetailPage.vue` per-entity log) show individual attempts but not how many consecutive failures preceded each row for that source — the number the tarpit/ban logic effectively counts toward a ban. User wants this surfaced as a "strike" number: **the Nth failed attempt from that `peer_ip`, since the last successful login** (resets to 1 right after any success). Rows that succeeded show no strike number.

Two ways to get this were discussed:
1. **Compute at query time** — SQL window functions, no schema change, retroactively correct for existing rows.
2. **Persist actual tarpit trigger_count** — thread `TarpitEntry.fail_count`/`trigger_count` (in-memory, `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`) through `ConnectionLog::create()` at write time — a "log of how it was back then," survives ban-logic changes/window tweaks.

User likes both eventually but wants **only the computed version scoped now**.

## Algorithm (computed strike number)

Single-pass window-function query, no recursive CTE, partitioned by `peer_ip`:

```sql
WITH marked AS (
  SELECT *,
         SUM(CASE WHEN success THEN 1 ELSE 0 END) OVER (
           PARTITION BY peer_ip ORDER BY started_at
           ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
         ) AS success_epoch
  FROM connection_logs
)
SELECT *,
       CASE WHEN success THEN NULL
            ELSE ROW_NUMBER() OVER (PARTITION BY peer_ip, success_epoch ORDER BY started_at)
       END AS strike_number
FROM marked
```

- `success_epoch` = count of prior successes for that `peer_ip` (excludes current row) → identifies the "streak" since the last success.
- `strike_number` = ordinal position of this failed row within its streak; `NULL` for successful rows and for rows with `peer_ip IS NULL`.
- Computed over the *whole table* per `peer_ip`, then existing filters/pagination applied on top in an outer query — correctness requires the full history per peer_ip, not just the paginated slice.

Grouping key: **`peer_ip` only** (mirrors the existing `count_recent_failures_for_peer_ip` helper already used by the tarpit engine — `user_id` is frequently `NULL` on failed attempts before a key is resolved, so it's not a reliable grouping key).

## Changes

### `crates/tunnel2tunnel-core/src/models/connection_log.rs`
- Add `pub strike_number: Option<i64>` to `ConnectionLog` struct (fetched via `sqlx::FromRow`, not written to by `create()`).
- Wrap the `SELECT * FROM connection_logs` in `search()` and `list_for_entity()` with the `marked`/window-function CTE shown above, applying existing `WHERE`/`ORDER BY`/`LIMIT`/`OFFSET` in the outer query. The `total` count query in `search()` is unaffected (doesn't need strike_number).

### `crates/tunnel2tunnel-web/src/routes/entities.rs`
- Add `pub strike_number: Option<i64>` to `ConnLogResponse`, populate from `ConnectionLog::strike_number` in the `From` impl. Shared by both `list_connection_logs` (entity page) and `search_connection_logs` (`tarpit.rs`, admin page) since they both build `ConnLogResponse` from this one struct.

### `frontend/src/api/admin.ts`
- Add `strike_number: number | null` to the `ConnLog` interface.

### `frontend/src/pages/AdminConnectionLogsPage.vue`
- Add a "Strike" `<th>`/`<td>` column (near Result/Reason), showing `l.strike_number ?? '—'`.

### `frontend/src/pages/EntityDetailPage.vue`
- Same column added to its connection-log table (lines ~660-680), same rendering.

## Verification
- `cargo test -p tunnel2tunnel-core` / `-p tunnel2tunnel-web` (existing tarpit/pagination unit tests must still pass).
- Manually run backend + frontend (`cargo run -p t2t` per CLAUDE.md run instructions), trigger a few failed SSH auths from the same peer_ip followed by a success, confirm strike numbers increment 1,2,3... and reset to none after the success in both the admin log page and an entity's detail page log tab.

## Not in scope (follow-up, if wanted later)
- Persisting the real in-memory `trigger_count` from the tarpit engine into a DB column (migration + threading through `ConnectionLog::create()` and the SSH auth path) — kept out per user's "for now" scoping.
