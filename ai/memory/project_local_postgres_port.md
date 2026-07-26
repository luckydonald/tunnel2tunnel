---
name: project_local_postgres_port
description: "Local t2t-pg podman container maps host port 5433, not 5432 — plus a separate Docker (not podman) postgres commonly squats 5432 on this machine"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4d8f0732-ba86-40c0-a1c7-3306e46d69ff
  modified: 2026-07-26T17:05:19.821Z
---

The `t2t-pg` podman container (per `CLAUDE.md`'s "Running locally" section) is often already running long-term on this dev machine, mapped to **host port 5433**, not the container-internal 5432 (`podman port t2t-pg` confirms). `DATABASE_URL=postgres://t2t:t2t_secret@localhost:5432/...` (the default many test files fall back to, e.g. `crates/t2t/tests/tarpit_e2e.rs::test_pool`) will silently hit a *different* Postgres if one happens to be listening on 5432 instead of failing to connect.

**Why this matters:** on 2026-07-26 there was also a separate `docker` (Docker Engine, not podman — this machine runs both) container `ecosystemng-postgres_db-1` (`postgres:16`) bound to `0.0.0.0:5432`, unrelated to this project. Auth failed against it with the t2t credentials, which looked like a broken DB setup rather than a port collision.

**How to apply:** before assuming Postgres is unreachable/misconfigured, run `podman port t2t-pg` to get the real host port, and `docker ps` / `ss -ltnp | grep 5432` to check for an unrelated container squatting 5432. Use the confirmed port explicitly in `DATABASE_URL` rather than relying on the 5432 default baked into test files.
