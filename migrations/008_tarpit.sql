-- ============================================================================
-- 1. Extend connection_logs: split failure_reason into fail_reason/success_reason,
--    derive `success`, add tarpit-related columns.
-- ============================================================================

ALTER TABLE connection_logs RENAME COLUMN failure_reason TO fail_reason;
ALTER TABLE connection_logs ADD COLUMN success_reason TEXT;

-- Backfill BEFORE adding the mutual-exclusivity CHECK so existing rows satisfy it.
UPDATE connection_logs
   SET success_reason = 'correct login',
       fail_reason    = NULL
 WHERE login_succeeded = true;

UPDATE connection_logs
   SET fail_reason = COALESCE(fail_reason, 'unspecified failure')
 WHERE login_succeeded = false;

ALTER TABLE connection_logs
  ADD CONSTRAINT connection_logs_fail_reason_check
  CHECK (fail_reason IS NULL OR fail_reason IN (
    'password auth not supported',
    'unsupported auth method',
    'unknown key',
    'key expired',
    'key expired (valid_until)',
    'entity not found',
    'entity expired',
    'ip blocked by whitelist',
    'ip banned',
    'user banned',
    'rate limited',
    'tarpitted - gave up',
    'unspecified failure'
  ));

ALTER TABLE connection_logs
  ADD CONSTRAINT connection_logs_success_reason_check
  CHECK (success_reason IS NULL OR success_reason IN (
    'correct login',
    'admin reset ban'
  ));

-- Exactly one of the two reason columns must be non-NULL.
ALTER TABLE connection_logs
  ADD CONSTRAINT connection_logs_reason_xor_check
  CHECK (num_nonnulls(fail_reason, success_reason) = 1);

ALTER TABLE connection_logs DROP COLUMN login_succeeded;
ALTER TABLE connection_logs
  ADD COLUMN success BOOLEAN GENERATED ALWAYS AS (success_reason IS NOT NULL) STORED;

ALTER TABLE connection_logs ADD COLUMN attempted_password TEXT;
ALTER TABLE connection_logs ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD COLUMN tarpit_method TEXT
  CHECK (tarpit_method IS NULL OR tarpit_method IN ('banner_drip', 'slow_auth', 'fake_shell'));

CREATE INDEX connection_logs_peer_ip_idx ON connection_logs (peer_ip);
CREATE INDEX connection_logs_success_idx ON connection_logs (success);
CREATE INDEX connection_logs_user_id_idx ON connection_logs (user_id);
CREATE INDEX connection_logs_fail_reason_idx ON connection_logs (fail_reason) WHERE fail_reason IS NOT NULL;

-- ============================================================================
-- 2. Admin-managed ban/tarpit rules — by peer_ip OR by user, optional expiry.
-- ============================================================================

CREATE TABLE ban_rules (
    id            UUID PRIMARY KEY DEFAULT uuidv7(),
    scope_type    TEXT NOT NULL CHECK (scope_type IN ('peer_ip', 'user')),
    peer_ip       TEXT,
    user_id       UUID REFERENCES users(id) ON DELETE CASCADE,
    reason        TEXT,
    active_until  TIMESTAMPTZ,
    created_by    UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
      (scope_type = 'peer_ip' AND peer_ip IS NOT NULL AND user_id IS NULL) OR
      (scope_type = 'user'    AND user_id IS NOT NULL AND peer_ip IS NULL)
    )
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON ban_rules
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

CREATE INDEX ban_rules_peer_ip_idx ON ban_rules (peer_ip) WHERE peer_ip IS NOT NULL;
CREATE INDEX ban_rules_user_id_idx ON ban_rules (user_id) WHERE user_id IS NOT NULL;
CREATE INDEX ban_rules_active_until_idx ON ban_rules (active_until);

-- ============================================================================
-- 3. Admin-configurable global thresholds — reuse existing settings kv table.
-- ============================================================================

INSERT INTO settings (key, value) VALUES
  ('tarpit_threshold_count',          '5'),
  ('tarpit_threshold_window_seconds', '600'),
  ('tarpit_enabled',                  'true')
ON CONFLICT (key) DO NOTHING;
