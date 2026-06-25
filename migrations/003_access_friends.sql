-- entity_access: who can connect to which entity
CREATE TABLE entity_access (
    id                UUID PRIMARY KEY DEFAULT uuidv7(),
    owner_entity_id   UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    subject_type      TEXT NOT NULL CHECK(subject_type IN (
                        'entity', 'all_mine', 'all_user_entities', 'public_lite')),
    subject_entity_id UUID REFERENCES entities(id) ON DELETE CASCADE,
    subject_user_id   UUID REFERENCES users(id) ON DELETE CASCADE,
    hostname          TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON entity_access
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

-- friendships between users; controls entity visibility
CREATE TABLE friendships (
    id               UUID PRIMARY KEY DEFAULT uuidv7(),
    from_user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    to_user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status           TEXT NOT NULL DEFAULT 'pending'
                       CHECK(status IN ('pending', 'accepted', 'declined')),
    visibility_grant TEXT NOT NULL DEFAULT 'none'
                       CHECK(visibility_grant IN ('none', 'clients', 'servers', 'all')),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(from_user_id, to_user_id)
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON friendships
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();

-- per-entity explicit visibility overrides within a friendship
CREATE TABLE friendship_entity_grants (
    id            UUID PRIMARY KEY DEFAULT uuidv7(),
    friendship_id UUID NOT NULL REFERENCES friendships(id) ON DELETE CASCADE,
    entity_id     UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(friendship_id, entity_id)
);

CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON friendship_entity_grants
    FOR EACH ROW EXECUTE FUNCTION set_timestamps();
