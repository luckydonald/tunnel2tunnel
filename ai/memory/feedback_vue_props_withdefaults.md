---
name: feedback_vue_props_withdefaults
description: withDefaults optional props still type as possibly-undefined in template; use a computed normalizer
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d15155a6-419b-41d2-a891-d244de162112
---

When a Vue 3 prop is typed as optional (`prop?: T[]`) and given a runtime default via `withDefaults`, the Vue Language Tools template type checker may still treat it as `T[] | undefined`.

**Why:** The VLS doesn't always narrow through `withDefaults` defaults for optional array/object props.

**How to apply:** Add a normalizing computed in `<script setup>`:
```typescript
const items = computed(() => props.myOptionalProp ?? [])
```
Use `items` (not `props.myOptionalProp`) in both the script and template to avoid TS18048 errors. Also note: the correct Vue keyboard modifier for Escape is `@keyup.esc`, not `@keyup.escape`.
