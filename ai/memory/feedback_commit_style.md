---
name: feedback_commit_style
description: "lplp-pipbuck commit style rules active for this project — fold patterns, plan handling, pending-commit.md flow"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d15155a6-419b-41d2-a891-d244de162112
  modified: 2026-07-23T12:00:08.764Z
---

The user uses the lplp-pipbuck commit style (invoke via `/commit-with-lplp-style`). Always active once enabled in a session.

**Why:** Keeps AI-auto-commits out of code history; readable branch log.

**How to apply:**

- Write message to `ai/git/pending-commit.md` first (delete stale file with `rm ... || echo 'was gone'`), then commit with `-F ai/git/pending-commit.md`. Never inline the message.
- Message format: `[where] topic: ai: Run: Short summary.` — where is `[db]`, `[backend]`, `[frontend]`, `[coolify]`, etc.; multiple as `[db|backend|frontend]`.
- Before committing, check the last 2 commits. Fold `ai: updated prompt`, `ai: agent <id> results`, `ai: record memory <slug>` into the implementation commit via `git reset --soft HEAD~N`.
- Keep plan commits (`ai: save plan NNN_...`) separate **when multiple versions of the same plan number exist** (they record evolution). A lone plan commit with no follow-up updates may be folded.
- Stage by explicit path — never `git add .` or `git add -A`. Never stage `ai/git/pending-commit.md` (gitignored).
- Commit after every completed task automatically without asking again.

**Cleaning up when plan-revision commits are interleaved with prompt/decision/agent-result commits (not just consecutive):** `git reset --soft` only works on a contiguous tail. When a plan went through several real revisions (each worth its own `ai: Plan:`/`ai: Plan update:`) with prompt/decision/agent-result auto-commits scattered between them, use `git rebase -i <base>` instead:
1. Write one short message file per kept revision to `ai/git/rebase-msg-<sha>.md` (content: the final one-line renamed message, e.g. `[where] topic: ai: Plan update: ...`).
2. Build a todo via `GIT_SEQUENCE_EDITOR=/path/to/script.sh git rebase -i <base>`, where the script writes a todo like: `pick <first-auto-commit-in-group>` then `fixup <every other commit in that group, including the real plan commit whose content you want>` then `exec git commit --amend -F ai/git/rebase-msg-<sha>.md`. Repeat per group. Fixup always folds *backward* into the preceding `pick`/`fixup` in the todo — so to fold commits *forward* into a later one, list the earlier auto-commits as the `pick`/leading `fixup`s and put the commit whose content you want last in that group, then rename with `exec`.
3. If there are uncommitted working-tree changes (e.g. mid-implementation), `git stash push -u -m "..."` before the rebase and `git stash pop` after — rebase requires a clean tree.

**Why:** confirmed working this session cleaning up 12 interleaved commits (5 plan-content commits mixed with prompt/decision/agent-result auto-commits) down to 4 correctly-ordered `Plan:`/`Plan update:` commits, preserving genuine revision history.
