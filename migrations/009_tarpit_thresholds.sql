-- ============================================================================
-- Replace the single global tarpit_threshold_count/window_seconds setting
-- with a table of independently-configurable count+window rules — any one
-- of which can trigger a ban. tarpit_enabled stays in `settings` as a global
-- kill switch, orthogonal to individual rules.
-- ============================================================================

CREATE TABLE tarpit_thresholds (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    fail_count      INT NOT NULL CHECK (fail_count > 0),
    window_seconds  BIGINT NOT NULL CHECK (window_seconds > 0),
    enabled         BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON tarpit_thresholds
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

-- Backfill one row from the existing global settings (or defaults 5/600).
INSERT INTO tarpit_thresholds (fail_count, window_seconds, enabled)
SELECT
    COALESCE((SELECT value FROM settings WHERE key = 'tarpit_threshold_count')::INT, 5),
    COALESCE((SELECT value FROM settings WHERE key = 'tarpit_threshold_window_seconds')::BIGINT, 600),
    true;

DELETE FROM settings WHERE key IN ('tarpit_threshold_count', 'tarpit_threshold_window_seconds');
