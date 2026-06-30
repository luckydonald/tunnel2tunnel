# Memory Index

- [Commit style (lplp-pipbuck)](feedback_commit_style.md) — fold patterns, plan handling, pending-commit.md flow; always active once enabled
- [Vue `withDefaults` + optional props](feedback_vue_props_withdefaults.md) — use a computed normalizer; `@keyup.esc` not `.escape`
- [PostgreSQL 18 data dir](project_postgres18_data_dir.md) — mount `/var/lib/postgresql`, not `/data`; use `pg_isready -q`
- [Port discovery architecture](project_discovery_architecture.md) — three-state model, migration 005–006, two-query pattern
- [russh auth types + entity.name](russh_method_set.md) — `MethodKind`/`MethodSet` at `russh::` root (not `russh::auth`); `entity.name` is `Option<String>`, unwrap before `%` in tracing
- [Coolify compose pattern](coolify_compose_pattern.md) — magic vars, custom env vars, volumes; update 5 files when adding a new env var
- [SSH host key persistence](ssh_key_persistence.md) — ssh-key 0.7 encrypt/decrypt API; load-or-generate pattern; mount `/data` as volume
