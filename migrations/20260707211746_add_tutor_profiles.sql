-- Add migration script here
CREATE TABLE tutor_profiles (
    id              UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    user_id         UUID             NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id    UUID             NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    bio             TEXT,
    credentials     TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    subjects        TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    years_experience INTEGER,
    profile_photo_key TEXT,
    verified_by_workspace BOOLEAN    NOT NULL DEFAULT FALSE,
    verified_at     TIMESTAMPTZ,
    verified_by_user_id UUID         REFERENCES users(id) ON DELETE SET NULL,
    avg_rating      NUMERIC(3, 2),
    rating_count    INTEGER          NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, workspace_id),
    CHECK(char_length(bio) <= 2000),
    CHECK(years_experience IS NULL OR years_experience BETWEEN 0 AND 60)
);

CREATE INDEX idx_tutor_profiles_workspace ON tutor_profiles(workspace_id);
CREATE INDEX idx_tutor_profiles_subjects  ON tutor_profiles USING GIN (subjects);
CREATE INDEX idx_tutor_profiles_rating    ON tutor_profiles(workspace_id, avg_rating DESC NULLS LAST);

CREATE TRIGGER set_updated_at_tutor_profiles
BEFORE UPDATE ON tutor_profiles
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();