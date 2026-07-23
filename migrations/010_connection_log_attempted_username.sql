-- The SSH username string the client actually sent (e.g. "root", "admin") is
-- distinct from `user_id`, which only resolves once a key fingerprint maps to
-- a registered entity/user. Surfacing the raw attempted username lets the
-- admin connection-log UI show what was typed even when it never resolves.
ALTER TABLE connection_logs ADD COLUMN attempted_username TEXT;
