CREATE TABLE users (
  id            UUID        PRIMARY KEY DEFAULT uuidv7(),
  username      TEXT        NOT NULL UNIQUE,
  email         TEXT,
  password_hash TEXT        NOT NULL,
  is_admin      BOOL        NOT NULL DEFAULT false,
  is_locked     BOOL        NOT NULL DEFAULT false,
  description   TEXT,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at    TIMESTAMPTZ
);

CREATE TRIGGER users_timestamps
  BEFORE INSERT OR UPDATE ON users
  FOR EACH ROW EXECUTE FUNCTION set_timestamps();

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT INTO settings (key, value) VALUES
  ('signup_enabled',    'false'),
  ('fail2ban_log_path', '');
