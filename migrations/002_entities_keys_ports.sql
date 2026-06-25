CREATE TABLE entities (
  id           UUID        PRIMARY KEY DEFAULT uuidv7(),
  user_id      UUID        NOT NULL REFERENCES users(id),
  type         TEXT        NOT NULL CHECK(type IN ('server', 'client')),
  name         TEXT,
  description  TEXT,
  ip_whitelist TEXT,
  valid_until  TIMESTAMPTZ,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at   TIMESTAMPTZ
);

CREATE TRIGGER entities_timestamps
  BEFORE INSERT OR UPDATE ON entities
  FOR EACH ROW EXECUTE FUNCTION set_timestamps();

CREATE TABLE ssh_keys (
  id          UUID        PRIMARY KEY DEFAULT uuidv7(),
  entity_id   UUID        NOT NULL REFERENCES entities(id),
  algorithm   TEXT        NOT NULL,
  key_data    TEXT        NOT NULL,
  comment     TEXT,
  fingerprint TEXT        NOT NULL UNIQUE,
  name        TEXT,
  valid_until TIMESTAMPTZ,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at  TIMESTAMPTZ
);

CREATE TRIGGER ssh_keys_timestamps
  BEFORE INSERT OR UPDATE ON ssh_keys
  FOR EACH ROW EXECUTE FUNCTION set_timestamps();

CREATE TABLE entity_ports (
  id          UUID    PRIMARY KEY DEFAULT uuidv7(),
  entity_id   UUID    NOT NULL REFERENCES entities(id),
  enabled     BOOL    NOT NULL DEFAULT true,
  local_port  INTEGER NOT NULL,
  proxy_port  INTEGER NOT NULL,
  name        TEXT,
  description TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER entity_ports_timestamps
  BEFORE INSERT OR UPDATE ON entity_ports
  FOR EACH ROW EXECUTE FUNCTION set_timestamps();
