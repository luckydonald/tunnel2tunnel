ALTER TABLE tarpit_thresholds ADD COLUMN deleted_at TIMESTAMPTZ;
ALTER TABLE ban_rules ADD COLUMN deleted_at TIMESTAMPTZ;
