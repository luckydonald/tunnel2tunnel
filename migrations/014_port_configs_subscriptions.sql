-- Replace the entity_ports/entity_port_discovery_rules two-sided config model
-- with a single-declaration port_configs table plus an explicit M2M
-- port_subscriptions join table. See ai plan "robust-bubbling-ember" for the
-- full rationale.

-- entity_ports -> port_configs: the single declaration "entity X offers this
-- port as a service." No longer tied to the entity being "a server," and no
-- longer mirrored by a client-side row.
ALTER TABLE entity_ports RENAME TO port_configs;
ALTER TABLE port_configs DROP COLUMN server_entity_id;
UPDATE port_configs SET name = 'Unnamed Service' WHERE name IS NULL;
ALTER TABLE port_configs ALTER COLUMN name SET NOT NULL;

-- entity_port_discovery_rules is replaced entirely by port_subscriptions.
DROP TABLE entity_port_discovery_rules;

CREATE TABLE port_subscriptions (
    id                    UUID PRIMARY KEY DEFAULT uuidv7(),
    port_config_id        UUID NOT NULL REFERENCES port_configs(id) ON DELETE CASCADE,
    subscriber_entity_id  UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    subscriber_local_port INTEGER NOT NULL,
    enabled               BOOL NOT NULL DEFAULT true,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (port_config_id, subscriber_entity_id)
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON port_subscriptions
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

-- entity_access gains optional port scoping: NULL = grant covers the whole
-- owner entity (today's behavior, unchanged); set = grant is scoped to
-- exactly that one port_config.
ALTER TABLE entity_access
    ADD COLUMN port_config_id UUID REFERENCES port_configs(id) ON DELETE CASCADE;
