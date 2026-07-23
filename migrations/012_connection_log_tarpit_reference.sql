-- Records which action was actually taken against a connection attempt
-- (trap or ban) and which rule decided it — a tarpit_thresholds row for an
-- automatic decision, or a ban_rules row for an admin-triggered one (the
-- admin themselves is reached via ban_rules.created_by). The two references
-- are mutually exclusive: a given ban event has exactly one source.
ALTER TABLE connection_logs ADD COLUMN tarpit_action TEXT CHECK (tarpit_action IS NULL OR tarpit_action IN ('trap', 'ban'));
ALTER TABLE connection_logs ADD COLUMN tarpit_threshold_id UUID REFERENCES tarpit_thresholds(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD COLUMN banned_by_ban_rule_id UUID REFERENCES ban_rules(id) ON DELETE SET NULL;
ALTER TABLE connection_logs ADD CONSTRAINT connection_logs_ban_reference_xor_check
  CHECK (tarpit_threshold_id IS NULL OR banned_by_ban_rule_id IS NULL);
