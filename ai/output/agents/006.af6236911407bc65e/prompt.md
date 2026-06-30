Explore the tunnel2tunnel backend codebase at /home/user/git/luckydonald/tunnel2tunnel. I need to understand:

1. The entity_ports database schema — what fields does an entity_port have (look in migrations/002_entities_keys_ports.sql)
2. The ports API routes — GET /api/entities/{id}/ports, what data is returned
3. The entity_access table — what fields, especially anything about port forwarding
4. The API routes for access — GET /api/entities/{id}/access
5. Any existing backend logic around port forwarding in SSH tunnels (in crates/tunnel2tunnel-ssh/)

Search the codebase thoroughly: migrations/, crates/tunnel2tunnel-web/src/ (for route handlers), crates/tunnel2tunnel-core/src/ (for models).