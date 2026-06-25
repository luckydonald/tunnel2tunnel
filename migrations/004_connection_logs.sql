CREATE TABLE connection_logs (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    entity_id       UUID REFERENCES entities(id) ON DELETE SET NULL,
    peer_ip         TEXT,
    key_fingerprint TEXT,
    login_succeeded BOOL NOT NULL,
    failure_reason  TEXT,
    ssh_flags       TEXT,
    ports_requested TEXT,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at        TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON connection_logs
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

CREATE INDEX connection_logs_entity_id_idx ON connection_logs(entity_id);
CREATE INDEX connection_logs_started_at_idx ON connection_logs(started_at DESC);
