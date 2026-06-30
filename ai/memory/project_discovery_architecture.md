---
name: project_discovery_architecture
description: "Port discovery system: three-state model, entity_port_discovery_rules table, server_entity_id on EntityPort"
metadata: 
  node_type: memory
  type: project
  originSessionId: d15155a6-419b-41d2-a891-d244de162112
---

Client entities have an auto-discovery system for reachable server ports (implemented in migrations 005–006).

**Three states** for each (client, server_port) pair:
- `auto` (null) — no DB row; port included in SSH command by default based on live discovery
- `enabled` — `entity_port_discovery_rules` row (state='enabled') + a real `EntityPort` on the client; linked via `client_port_id`
- `disabled` — `entity_port_discovery_rules` row (state='disabled'); excluded from command

**`server_entity_id`** on `entity_ports` (migration 006): for client-side ports, the FK to the server entity whose SSH username appears in the `-L local:<server>:proxy` flag. Auto-set when pinning (state→enabled); optionally set when creating ports manually.

**Two-query pattern** in `list_reachable_for_client`:
1. `DISTINCT ON (e.id)` query for server entities + best-match hostname (correlated subquery ordered by specificity of access rule).
2. `WHERE ep.entity_id = ANY($1)` query for all their enabled ports + left-joined discovery state.

**Why two queries:** avoids N+1 and also avoids the cartesian explosion from a single JOIN across entities × ports × access rules.

See: [[feedback_commit_style]]
