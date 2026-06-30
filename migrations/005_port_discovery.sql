CREATE TABLE entity_port_discovery_rules (
    id                UUID    PRIMARY KEY DEFAULT uuidv7(),
    client_entity_id  UUID    NOT NULL REFERENCES entities(id)     ON DELETE CASCADE,
    server_port_id    UUID    NOT NULL REFERENCES entity_ports(id) ON DELETE CASCADE,
    -- 'enabled' = explicitly accepted (a matching EntityPort exists on the client)
    -- 'disabled' = explicitly excluded from auto-discovery
    state             TEXT    NOT NULL CHECK (state IN ('enabled', 'disabled')),
    -- set when state = 'enabled': the auto-created EntityPort on the client entity
    client_port_id    UUID    NULL     REFERENCES entity_ports(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL,
    UNIQUE (client_entity_id, server_port_id)
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON entity_port_discovery_rules
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();
