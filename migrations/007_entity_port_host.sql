-- Allow server-side ports to forward to a host other than localhost.
-- Default stays 'localhost' so existing ports are unaffected.
ALTER TABLE entity_ports
    ADD COLUMN host TEXT NOT NULL DEFAULT 'localhost';
