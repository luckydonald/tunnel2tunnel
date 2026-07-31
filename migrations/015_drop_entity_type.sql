-- entity.type ('server'/'client') is no longer a stored capability gate.
-- "Server"/"Client" become computed booleans (is_server/is_client) derived
-- from whether the entity owns any port_configs/port_subscriptions rows.
ALTER TABLE entities DROP COLUMN type;
