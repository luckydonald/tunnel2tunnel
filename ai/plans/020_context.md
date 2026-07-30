# Context

`SshCommandDisplay.vue` has two interactive features with no test coverage: hover-linking between the SSH command line and the manual/GUI-client config fields (`hoveredField`, `isFieldHovered`, driven by `user`/`host`/`port`/`key`), and copy-to-clipboard buttons in the manual-config rows (`copyValue`, `copiedField`). The frontend has **no test framework installed at all** — no vitest, no `@vue/test-utils`, no test script. This plan adds minimal test infra and a focused spec file for this component only.

## 1. Add test tooling (devDependencies + config)

- Add to `frontend/package.json` devDependencies: `vitest`, `@vue/test-utils`, `jsdom`, `@vitest/coverage-v8` not required (skip unless asked).
- Add script: `"test": "vitest run"`.
- Add `frontend/vitest.config.ts` (separate from `vite.config.ts` to avoid pulling in the Sentry/git-metadata build logic for tests): configure `plugins: [vue()]`, `resolve.alias['@']` matching `vite.config.ts`, `test: { environment: 'jsdom', globals: false }`.

## 2. Test file: `frontend/src/components/SshCommandDisplay.spec.ts`

Mount the component with `@vue/test-utils`, using minimal fixture props:
- `entity`: `{ id: 'entity-uuid', entity_type: 'client', ... }` (cast minimal fields needed, or import `Entity` type and satisfy it)
- `ports: []` (keep fixture simple — hover/copy tests don't need real port rows)
- `filename: 'test_key'`, `t2tHost: 'example.com'`, `t2tSshPort: 2222`

Mock `navigator.clipboard.writeText` via `Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } })` in a `beforeEach`, since jsdom doesn't implement Clipboard API.

Tests:
1. **Hover: command line → manual config.** Trigger `mouseenter` on the `user` `.cmd-flag` span inside `.cmd-text`, assert the manual-config row's `code.manual-field-code` for User gains `is-hovered`; trigger `mouseleave`, assert it's removed. Repeat the same pattern for `host`, `port`, and `key` fields (parametrize with a small loop/`it.each` over the four fields, locating elements by content/order rather than new test-ids, matching current markup — no test IDs exist, so select by `.cmd-text .cmd-flag` index or by text content).
2. **Hover: manual config → command line.** Trigger `mouseenter` on the manual-field-row's code element (e.g. Host), assert the corresponding `.cmd-flag` in `.cmd-text` gains `is-hovered`; `mouseleave` clears it.
3. **Hover mutual exclusivity.** Hovering one field doesn't mark a different field's elements as hovered (sanity check `hoveredField` is a single value).
4. **Copy button: writes correct value.** Click the User row's `.btn-copy`, assert `navigator.clipboard.writeText` called with `entity.id`. Repeat for Host (`t2tHost`) and Port (`String(t2tSshPort)`) and the key (`~/.ssh/{filename}`).
5. **Copy button: shows "Copied!" and reverts.** Use `vi.useFakeTimers()`; click a copy button, assert button text becomes `Copied!` and has `copied` class; advance timers by 2000ms (`vi.advanceTimersByTime(2000)`); assert it reverts to `Copy`. Restore real timers after.
6. **Copy button: independent per field.** Clicking one field's copy button doesn't mark a different field's button as copied (since `copiedField` is a single ref).

Use `await wrapper.vm.$nextTick()` after each interaction before assertions where needed.

## 3. Verification

- Run `cd frontend && npm install && npm run test` — all new tests pass.
- Run `npm run build` (`vue-tsc && vite build`) to confirm the new `vitest.config.ts`/spec file don't break the existing TS project/build (spec file excluded from `tsconfig.json`'s `include` is fine since vitest transpiles independently, but check `vue-tsc` doesn't choke on it — if it does, add `"exclude": ["src/**/*.spec.ts"]` to `tsconfig.json`).
