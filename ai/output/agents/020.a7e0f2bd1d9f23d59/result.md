```
Types:
- frontend/src/api/admin.ts:77 — `TarpitThreshold` — global auto-trigger rules
- frontend/src/api/admin.ts:88 — `TarpitThresholdParams` — threshold creation payload
- frontend/src/api/admin.ts:50 — `BanRule` — per-IP/user rule with soft-delete
- frontend/src/api/admin.ts:64 — `CreateBanRuleParams` — ban rule creation payload

Global Threshold Form:
- frontend/src/pages/AdminBanRulesPage.vue:128 — threshold state (fail_count, window_seconds, action)
- frontend/src/pages/AdminBanRulesPage.vue:226 — threshold creation form markup
- frontend/src/pages/AdminBanRulesPage.vue:134 — `handleAddThreshold()` handler

Per-IP/User Ban Rule Form:
- frontend/src/pages/AdminBanRulesPage.vue:24 — rule state (scope_type: peer_ip | user)
- frontend/src/pages/AdminBanRulesPage.vue:283 — ban rule creation form markup
- frontend/src/pages/AdminBanRulesPage.vue:44 — `handleAdd()` handler

Quick Ban Form (IP/User context):
- frontend/src/pages/AdminConnectionLogsPage.vue:76 — modal state (pre-determined scope from log)
- frontend/src/pages/AdminConnectionLogsPage.vue:204 — quick ban modal markup
- frontend/src/pages/AdminConnectionLogsPage.vue:90 — `confirmBan()` handler

Key difference:
Thresholds (global) = count + window + action; Ban rules (scoped) = scope_type + target (ip/user_id) + reason + active_until + action. Quick form auto-fills scope from log entry.
```