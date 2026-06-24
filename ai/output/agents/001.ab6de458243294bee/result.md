Perfect! Now I have a complete picture. Let me compile the findings:

## Summary: Project Structure & Conventions

This is **tunnel2tunnel**, a new SSH tunnel manager project that has adopted the **luckydonald/base** reusable git base. The project is in early stages with spec/init files but minimal actual code.

### Key Architecture

**°base Namespace Pattern**
- Everything base-specific is prefixed with `°base` (degree symbol) to prevent collisions
- Base files live in consuming repos as normal files, synced from `luckydonald/base`

**Directory Structure**

| Path | Purpose |
|---|---|
| `/ai/°base/` | Base repo's own AI artifacts (queries, plans, memory) |
| `/ai/plans/` | Project-specific plans (empty for now) |
| `/ai/references/` | External references (empty for now) |
| `/ai/skills/commit-with-lplp-style/` | Custom skill definitions |
| `/ai/tool-settings/settings.json` | Single source of truth for Claude + Codex settings |
| `.claude/settings.json` | Generated (do not edit directly) |
| `/scripts/°base/` | Helper scripts, tests, hooks |
| `/scripts/°base/ai/hooks/` | Event hooks (save-prompt, save-decision, save-plan, record-memory) |
| `/scripts/°base/ai/settings/sync.py` | Syncs settings & skills between tool formats |

### Conventions & Best Practices

**Commit Format (LPLP style)**
```
[where] component: ai: Run: Short summary.

Body with details.
```
- `[where]` examples: `[frontend]`, `[backend]`, `[docker]`, `[base]`
- Auto-commits (prompt/decision/plan) fold into preceding code commits by default
- Always write message to `ai/git/pending-commit.md` first, then pass via `-F` flag
- See `/ai/skills/commit-with-lplp-style/SKILL.md` for full rules (144 lines)

**AI Artifact Routing**
- Hooks auto-detect whether running in base repo (`ai/°base/`) or consuming repo (`ai/`)
- Detection: directory named `base` AND origin URL matches `luckydonald/base`
- For tunnel2tunnel (consuming repo), artifacts go to `ai/` not `ai/°base/`

**Settings & Skills Sync**
1. Edit canonical files: `ai/tool-settings/settings.json` and `ai/skills/*/SKILL.md`
2. Run `python3 scripts/°base/ai/settings/sync.py` to render into `.claude/` and `.codex/`
3. Pre-commit hook enforces sync with `--check` flag

**Testing**
```bash
uv run --project scripts/°base python -m unittest discover -s scripts/°base/tests -v
```

### Project Spec

See `/init.md` (170 lines) — detailed spec for a web-based SSH tunnel manager with:
- Admin/user authentication (with password reset, user management)
- Server/client key upload & management
- IP whitelisting (with gitignore-style rules, regex, wildcards)
- Cross-user sharing (friend lists, permissions)
- Soft deletes, audit logging, port forwarding UI
- SSH command generation & audit trails

**Technology hints from spec:**
- Docker Compose + Coolify support
- Admin dashboard at `/login`
- Server/client entity management
- Database with soft-deletes
- SSH key authentication

### Critical Notes for New Development

1. **Don't edit `.claude/settings.json` directly** — always modify `/ai/tool-settings/settings.json` and run sync
2. **Stage only task-specific files** — never stage `ai/git/pending-commit.md` (gitignored)
3. **Use `lplp-pipbuck` commit style** — activate the `/commit-with-lplp-style` skill when ready
4. **Understand hook order**: deny-list fires before PermissionRequest hook (see `/ai/°base/query.md` for discussion)
5. **Pre-commit hooks** block Co-Authored-By patterns and enforce memory file deletion markers

### Files to Read When Building

- `/CLAUDE.md` — project-level guidance (sync commands, architecture)
- `/ai/tool-settings/settings.json` — permissions whitelist, hooks config (215 lines)
- `/ai/skills/commit-with-lplp-style/SKILL.md` — full commit conventions (144 lines)
- `/ai/°base/AGENTS.md` — base repo's own documentation