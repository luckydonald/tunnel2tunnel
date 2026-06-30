---
name: coolify_compose_pattern
description: How Coolify docker-compose magic vars work and where to add new env/volume vars
metadata: 
  node_type: memory
  type: project
  originSessionId: 4e03ed8c-f268-407f-a241-415199236b9b
---

## Coolify magic vars in docker-compose.coolify.yml

Coolify auto-injects these variables — do not set them manually:
- `SERVICE_FQDN_TUNNEL2TUNNEL_3000` — public FQDN routed to port 3000
- `SERVICE_USER_POSTGRES` / `SERVICE_PASSWORD_POSTGRES` — DB credentials
- `SERVICE_USER_ADMIN` / `SERVICE_PASSWORD_ADMIN` — admin bootstrap creds

**Custom env vars** (e.g. `SSH_T2T_KEY_PASSWORD`) must be added manually in the Coolify UI under the service's environment variables tab. Wire them with `${VAR_NAME:-}` in `docker-compose.coolify.yml` so the container still starts when the var is unset.

**Volumes** must be declared in both the service's `volumes:` list and the top-level `volumes:` block — same as plain Docker Compose.

**Where to update when adding a new env var or volume:**
1. `docker-compose.yml` (dev)
2. `docker-compose.coolify.yml` (prod)
3. `.env.example` (local dev template)
4. `CLAUDE.md` env var table
5. `README.md` env var table

**Why:** The two compose files diverge intentionally — Coolify uses its own magic vars for DB/admin credentials. Missing a file leads to silent differences between dev and prod.
