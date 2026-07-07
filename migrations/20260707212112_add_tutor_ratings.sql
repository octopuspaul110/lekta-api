-- Add migration script here
CREATE TABLE tutor_ratings (
    id              UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    tutor_profile_id UUID            NOT NULL REFERENCES tutor_profiles(id) ON DELETE CASCADE,
    student_user_id UUID             NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id    UUID             NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    rating          INTEGER          NOT NULL CHECK (rating BETWEEN 1 AND 5),
    comment         TEXT,
    class_id        UUID             REFERENCES classes(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    UNIQUE(tutor_profile_id, student_user_id, class_id),
    CHECK(char_length(comment) <= 1000)
);

CREATE INDEX idx_tutor_ratings_profile ON tutor_ratings(tutor_profile_id, created_at DESC);
CREATE INDEX idx_tutor_ratings_student ON tutor_ratings(student_user_id);

CREATE TRIGGER set_updated_at_tutor_ratings
BEFORE UPDATE ON tutor_ratings
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();