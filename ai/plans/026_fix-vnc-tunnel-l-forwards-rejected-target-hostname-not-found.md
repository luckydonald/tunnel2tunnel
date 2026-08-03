# Fix: VNC tunnel `-L` forwards rejected ("target hostname not found")

## Context

`ai/errors/8.ssh.txt` / `8.t2t.txt` show a client running `ssh -L 5951:m1n:5900 -L 5952:PC6:5900 ...`. Auth succeeds, but every `direct-tcpip` channel open (for both `m1n` and `PC6`) is rejected server-side with:

```
direct-tcpip: rejected — target hostname not found (not a UUID or known alias) host="m1n" port=5900
```

which the SSH client surfaces generically as `channel 5: open failed: administratively prohibited: Rejected`.

This is the *exact same failure class* already fixed in commit `f05b20f` (2026-08-01): `resolve_target_entity` (`crates/tunnel2tunnel-ssh/src/lib.rs:1437`) was extended with a third fallback — resolve a plain, unambiguous `entities.name` via `Entity::find_unique_id_by_name` (`crates/tunnel2tunnel-core/src/models/entity.rs:47`) — specifically because the frontend's generated `-L` command uses the entity's *display name* as the SSH hostname, not a UUID or a registered `entity_access.hostname` alias.

Investigation (confirmed via `git merge-base --is-ancestor f05b20f origin/mane` → **not an ancestor**) shows the real problem: **that fix was never pushed.** Local `mane` is 12 commits ahead of `origin/mane` (tip `4d28ee5`), and `f05b20f` plus everything after it sits only locally. Whatever builds/deploys from `origin/mane` is still running the old two-way resolver (UUID or `entity_access.hostname` alias only), so display-name forwards like `m1n`/`PC6` can never resolve — confirmed by the user: both entity names are exact, case-correct, and unambiguous, so the current (local) resolver code would in fact accept them.

No code fix is needed — it already exists and is exercised by `crates/tunnel2tunnel-core/tests/entity_models.rs`. This is a **deploy-lag** bug, not a logic bug.

## Plan

1. **Push the 12 pending commits to `origin/mane`.**
   `git push origin mane` — brings `f05b20f` (and the tunnel_e2e strengthening commits after it) to the remote the deploy pipeline builds from. Fast-forward push, no force needed (confirm no divergence first: `git fetch && git status -sb` should show only "ahead", not "diverged").

2. **Trigger/verify redeploy on the production host.**
   Confirm however this repo is deployed (Coolify, per `CLAUDE.md`) either auto-deploys on push to `mane`, or manually trigger a redeploy so the new binary is what's actually running on `tunnel2tunnel-d5oteit4omkq577f6j4r3rd9.h1.bn-x.de`. This step needs the user's confirmation/access — flag it, don't assume auto-deploy.

3. **Verify the fix end-to-end.**
   Re-run the same `-L m1n:5900 -L PC6:5900` SSH command against production and confirm both port-listeners get real connections through instead of `administratively prohibited`. If it still fails post-deploy, re-pull `t2t.log` and check whether resolution now gets past `resolve_target_entity` into the port_config/subscription/access checks (a different, later rejection reason) — that would point to a *second*, distinct problem, not this one.

4. **Sanitize `ai/errors/8.ssh.txt` and `ai/errors/8.t2t.txt`.**
   Follow the existing placeholder convention from `ai/errors/7.t2t.md` (TEST-NET-3 IPs, `<redacted-uuid>`, generic example domain) and apply it directly to both `8.*` files in place (unlike `7.ssh.txt`, which was left with real data — this task explicitly asks for the `8.*` files to be cleaned):
   - Domain `tunnel2tunnel-d5oteit4omkq577f6j4r3rd9.h1.bn-x.de` → a generic placeholder (e.g. `tunnel2tunnel-example.example.de`)
   - Real server IP `159.69.185.51` and client `peer_ip`s (`124.167.20.72`, `36.137.38.119`, `87.123.1.20`) → `203.0.113.x` / `198.51.100.x` (TEST-NET placeholders)
   - Entity UUID `019f15ec-fb3b-7f41-ba4b-84a0baa6b3cd` and `conn_id` UUIDs → `<redacted-uuid>`
   - Key fingerprint `SHA256:5thR88M+9XjHy7q7qRJooValOgq1Ll7vK8j4WeQ1S+s` → `SHA256:<redacted>`
   - Entity/host names `m1n`/`PC6`, and generic local paths (`/home/user/...`) can stay — they're not personally identifying.

## Notes

- No source changes in this plan — steps 1–2 are git/deploy operations, step 4 is doc cleanup. Per commit-style rules, step 4's file edits should be committed normally (not part of the lplp auto-commit fold patterns) once done.
- Step 2 is the one genuinely irreversible/external action (production deploy) — confirm with the user before triggering it if it's not already automatic on push.
