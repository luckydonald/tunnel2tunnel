-- Store which server entity a client port tunnels to.
-- NULL is valid (e.g. server-side ports, or legacy client ports before this migration).
ALTER TABLE entity_ports
    ADD COLUMN server_entity_id UUID REFERENCES entities(id) ON DELETE SET NULL;
