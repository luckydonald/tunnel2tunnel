---
name: feedback_test_thoroughness
description: "User wants every code path automated-tested regardless of runtime cost, and assertions that check relationships/bounds, not just presence"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 4d8f0732-ba86-40c0-a1c7-3306e46d69ff
  modified: 2026-07-26T17:05:28.340Z
---

When adding automated tests for a set of parallel code paths (e.g. one behavior across several branches/variants), cover **all** of them, even the slow/annoying one — don't skip a path just because exercising it takes real wall-clock time (e.g. waiting out a 10s drip interval). Confirmed 2026-07-26: I initially skipped the banner-drip trap variant of an `ended_at`-stamping test to keep the suite fast; user explicitly said "I want them all tested, no matter what."

Also: when asserting on a value like a timestamp, don't stop at "is it set" — check its relationship to other known values too (e.g. `ended_at >= started_at`, and `ended_at - started_at >= <minimum expected delay for this path>`). User asked for this explicitly ("asserting that end is before start, and possibly a certain minimal distance apart"). This catches classes of bugs a mere `is_some()`/non-null check would miss (backwards timestamps, a loop that exits after 0 iterations instead of actually waiting).

**How to apply:** default to exhaustive path coverage and relational/bounded assertions in test additions for this user, even before being asked — treat "test this" as "test this thoroughly," not "add one smoke test."
