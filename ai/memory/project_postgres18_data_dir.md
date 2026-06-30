---
name: project_postgres18_data_dir
description: PostgreSQL 18 changed PGDATA layout; mount /var/lib/postgresql not /var/lib/postgresql/data
metadata: 
  node_type: memory
  type: project
  originSessionId: d15155a6-419b-41d2-a891-d244de162112
---

PostgreSQL 18 stores data in a version-specific subdirectory under `/var/lib/postgresql`. The volume should be mounted at `/var/lib/postgresql` (not `/var/lib/postgresql/data` as was conventional for PG ≤ 17).

**Why:** Mounting at `/data` conflicts with PG18's internal layout, causing the container to crash in ~1.5 seconds before healthchecks can fire.

**How to apply:** In `docker-compose.coolify.yml` (and any Docker compose file targeting PG18):
```yaml
volumes:
  - pg_data:/var/lib/postgresql   # NOT /var/lib/postgresql/data
```

Also: use `pg_isready -q` for healthchecks instead of `pg_isready -U $POSTGRES_USER -d $POSTGRES_DB` — Coolify may mangle `$VAR` references in shell strings, producing empty usernames.
