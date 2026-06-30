---
name: feedback_commit_style
description: "lplp-pipbuck commit style rules active for this project — fold patterns, plan handling, pending-commit.md flow"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d15155a6-419b-41d2-a891-d244de162112
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
