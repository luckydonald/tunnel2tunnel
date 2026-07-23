-- Each rule (automatic threshold or admin-created ban) now picks an action:
-- 'trap' (existing banner_drip/slow_auth/fake_shell tarpit) or 'ban' (instant
-- reject, no games). Defaults preserve current behavior for existing rows —
-- threshold rules always trapped before, admin ban_rules are meant to
-- actually block (the button was already called "Ban").
ALTER TABLE tarpit_thresholds ADD COLUMN action TEXT NOT NULL DEFAULT 'trap' CHECK (action IN ('trap', 'ban'));
ALTER TABLE ban_rules ADD COLUMN action TEXT NOT NULL DEFAULT 'ban' CHECK (action IN ('trap', 'ban'));
