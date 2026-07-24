## Report

**Schema** — `migrations/004_connection_logs.sql`: `ended_at TIMESTAMPTZ` (nullable), plus `started_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`. No `disconnected_at`/`disconnect_at` column exists; the field is named `ended_at`.

**Where `ended_at` is set (backend, `tunnel2tunnel-core`):**
- `crates/tunnel2tunnel-core/src/models/connection_log.rs:81-88` — `ConnectionLog::set_ended(pool, id)` runs `UPDATE connection_logs SET ended_at = NOW() WHERE id = $1`.

**Where it's actually invoked (`tunnel2tunnel-ssh`):**
- `crates/tunnel2tunnel-ssh/src/lib.rs:1030-1038`, inside `impl Drop for T2tHandler`: when the handler drops (SSH connection closed), if `self.log_id` is `Some`, it spawns a task calling `ConnectionLog::set_ended(&pool, log_id)`. This *is* correctly wired up — `ended_at` gets updated on real connection close, not left null.
  - `self.log_id` itself is only populated at `crates/tunnel2tunnel-ssh/src/lib.rs:343` (`Ok(log) => self.log_id = Some(log.id)`), i.e. only for connections where a `connection_logs` row was created (presumably on auth attempt). If `log_id` is `None` (e.g., pre-auth failure paths that don't create a log, or code paths that never assign it), `ended_at` never gets set for that row — but that's an intentional "no log row" case, not an update bug.
  - `crates/tunnel2tunnel-ssh/src/tarpit/banner_drip.rs:95` also calls `set_ended` for tarpit-drip connections that finish, `let _ = ConnectionLog::set_ended(&pool, log.id).await;` — errors are silently swallowed here (unlike the Drop path which at least logs a warning).

**No timezone issue and no duration-calc bug found**: `set_ended` uses Postgres `NOW()` (server-side, `TIMESTAMPTZ`) consistent with `started_at`'s `DEFAULT NOW()`, so both columns are populated by the same clock/timezone. There is no explicit "duration" computation anywhere in Rust or frontend — no `duration`/`elapsed` field or calc found in `connection_log.rs`, `entities.rs`, or the Vue pages.

**Frontend rendering — this is where the actual bug is:**
- `crates/tunnel2tunnel-web/src/routes/entities.rs:584,605` — the `/api/entities/{id}/logs` DTO does include and serialize `ended_at` (RFC3339 string).
- `frontend/src/pages/AdminConnectionLogsPage.vue:160-161` — correctly renders both:
  ```vue
  <td class="td-ts">{{ new Date(l.started_at).toLocaleString() }}</td>
  <td class="td-ts">{{ l.ended_at ? new Date(l.ended_at).toLocaleString() : '—' }}</td>
  ```
- `frontend/src/pages/EntityDetailPage.vue:660-684` (the per-entity "Connection Log" table shown on the Entity Detail page) has **no "Ended"/"Disconnected" column at all** — the `<thead>` only has Time / Peer IP / Fingerprint / Result / Reason / Tarpit (lines 662-669), and the single timestamp cell at line 673 only renders `l.started_at`. `ended_at` is fetched from the API into the `connLogs` array (it's part of the shared log DTO) but is never read or rendered anywhere in this component.

**Bug summary**: Backend correctly records and updates `ended_at` on connection close (via the `Drop` impl and tarpit banner-drip path), and the admin-wide connection-logs page (`AdminConnectionLogsPage.vue`) displays it correctly. The bug is that `EntityDetailPage.vue`'s per-entity connection log table simply omits the "ended/disconnected at" column/cell — a UI omission, not a backend recording bug. Fix would be adding an "Ended" `<th>` and a `<td>` rendering `l.ended_at ? new Date(l.ended_at).toLocaleString() : '—'` (mirroring lines 160-161 of the admin page) in `EntityDetailPage.vue` around line 663-673.