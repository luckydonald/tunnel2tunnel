# Connection-log date and tarpit filters

## Summary

Extend the admin connection-log browser with optional started/ended timestamp filters and independent filters for tarpit method and tarpit action. Per your correction, the Result dropdown remains unchanged; “Ban” and “Trap” are filtered through the new action selector.

## Implementation changes

- Add Started and Ended filter controls, each with `≥`, `≤`, and `between` modes:
  - Use local `datetime-local` inputs; convert supplied values to RFC 3339/UTC before calling the API.
  - A one-bound mode requires its one timestamp; `between` requires both timestamps. Prevent the search and show a clear validation message when required values are missing.
  - Ended-time comparisons naturally exclude still-open logs (`ended_at = NULL`).
- Extend `GET /api/admin/connection-logs` with optional inclusive bounds:
  - `started_at_gte`, `started_at_lte`, `ended_at_gte`, and `ended_at_lte`, as RFC 3339 timestamps.
  - Apply the same predicates to both the paginated row query and total-count query.
- Add a **Tarpit action** selector alongside the existing method selector. Both selectors provide:
  - Any (do not constrain the column)
  - Some … (column is non-null)
  - No … (column is null)
  - Each concrete value: all supported methods for method; `Trap` and `Ban` for action.
- Keep these filters composable with result, peer IP, user ID, text search, and each other. A `Ban` record can therefore be found by choosing “Some tarpit action” or specifically “Ban,” even though it has no tarpit method.

## Test plan

- Add core-model tests covering all four inclusive timestamp boundaries, including an open log excluded by an ended-time filter.
- Test method/action `Some`, `No`, and concrete-value filtering, including a ban record with `tarpit_action = ban` and `tarpit_method = NULL`.
- Verify pagination totals and returned rows use exactly the same combined filters.
- Run focused Rust tests and the frontend type check.

## Assumptions

- Timestamp comparisons are inclusive.
- No migration or new database index is needed for this scoped filter addition.
- The Result selector remains `Any result`, `Success`, and `Failure`; tarpit state is represented solely by the method/action filters.
