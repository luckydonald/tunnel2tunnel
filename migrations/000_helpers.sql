-- uuidv7() is built-in as of PostgreSQL 18 — no extension needed.

CREATE OR REPLACE FUNCTION set_timestamps()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    NEW.created_at := NOW();
  END IF;
  NEW.updated_at := NOW();
  RETURN NEW;
END;
$$;
