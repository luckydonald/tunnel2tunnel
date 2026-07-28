# Memory Index

- [Commit style (lplp-pipbuck)](feedback_commit_style.md) — fold patterns, plan handling, pending-commit.md flow; always active once enabled
- [Vue `withDefaults` + optional props](feedback_vue_props_withdefaults.md) — use a computed normalizer; `@keyup.esc` not `.escape`
- [PostgreSQL 18 data dir](project_postgres18_data_dir.md) — mount `/var/lib/postgresql`, not `/data`; use `pg_isready -q`
- [Port discovery architecture](project_discovery_architecture.md) — three-state model, migration 005–006, two-query pattern
- [russh auth types + entity.name](russh_method_set.md) — `MethodKind`/`MethodSet` at `russh::` root (not `russh::auth`); `entity.name` is `Option<String>`, unwrap before `%` in tracing
- [Coolify compose pattern](coolify_compose_pattern.md) — magic vars, custom env vars, volumes; update 5 files when adding a new env var
- [SSH host key persistence](ssh_key_persistence.md) — ssh-key 0.7 encrypt/decrypt API; load-or-generate pattern; mount `/data` as volume
- [Tarpit e2e test flake](project_tarpit_test_flake.md) — RESOLVED 2026-07-26: fix is binding the client socket's *source* address, not varying the destination
- [Local Postgres port](project_local_postgres_port.md) — `t2t-pg` podman maps host 5433 not 5432; a separate Docker container can squat 5432
- [Test thoroughness](feedback_test_thoroughness.md) — cover every path no matter runtime cost; assert relationships/bounds, not just presence
- [History-master replay guards](2026-07-20-history-master-replay-guards.md) — TODO: summarize this file.
