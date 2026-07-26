# Trap on first wrong attempt (fail_count = 1)

## Context

User wants a rule that traps on the very first failed login attempt, unsure if
that means setting a count of `0` or `1`. Investigation of the tarpit
threshold system (`tarpit_thresholds` table / `TarpitThreshold` model /
`record_failure()` in `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs`) confirms:

- Comparison logic (`crates/tunnel2tunnel-ssh/src/tarpit/mod.rs:186-188`) increments the
  tally *before* comparing: `tally.0 += 1; if tally.0 >= rule.fail_count { ... }`.
  So `fail_count = 1` already trips on the first recorded failure — no off-by-one.
- `fail_count = 0` is blocked at two layers and intentionally so:
  - DB: `CHECK (fail_count > 0)` (`migrations/009_tarpit_thresholds.sql:10`)
  - Frontend: `min="1"` on the input (`frontend/src/pages/AdminBanRulesPage.vue:231`)
- No existing test exercises the `fail_count = 1` boundary (existing tests use 2/3).

Confirmed with user: no backend/schema change needed. `fail_count = 1` is the
correct value to use today. Remaining work is closing the test gap and making
the UI copy clear that `1` (not `0`) is the "trap immediately" value.

## Changes

1. **Add boundary test** in `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs` test
   module (alongside `record_failure_bans_at_threshold` etc., ~line 557):
   a test using `rule(1, 60)` asserting `record_failure` returns `true` (banned)
   on the very first call, with `trigger_count == 1`. This locks in the
   "first failure traps immediately" behavior the user is relying on.

2. **Clarify UI copy** in `frontend/src/pages/AdminBanRulesPage.vue` around the
   `fail_count` input (line 231): update the placeholder or add adjacent
   helper text so admins know the minimum valid value is `1` and that it means
   "trap on the very first failed attempt" (not `0`). Small, localized copy
   change only — no logic change.

## Out of scope

- No DB migration, no CHECK constraint change, no allowing `fail_count = 0`.
- No new "pre-auth trap" mechanism (trapping before any auth attempt at all)
  — that's a different feature the user did not ask for; only mention if they
  bring it up again.

## Verification

- `cargo test -p tunnel2tunnel-ssh tarpit` — new test passes, existing tarpit
  tests still pass.
- Manually create a global rule with `fail_count = 1` via the Ban Rules page
  and confirm a single failed SSH auth attempt from a fresh peer IP triggers
  the trap (check `connection_logs.tarpit_action` / server logs).
