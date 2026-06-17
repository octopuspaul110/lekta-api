-- Add migration script here
CREATE TABLE assignments(
    id                      UUID    PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    workspace_id            UUID                NOT NULL REFERENCES workspaces(id)       ON DELETE CASCADE,
    channel_id              UUID                NOT NULL REFERENCES channels(id)         ON DELETE CASCADE,
    creator_user_id         UUID                NOT NULL REFERENCES users(id)            ON DELETE RESTRICT,
    title                   TEXT                NOT NULL,
    description             TEXT                NOT NULL DEFAULT    '',
    attachments             JSONB               NOT NULL DEFAULT    '[]'::jsonb,
    max_score               INTEGER             NOT NULL DEFAULT    100,
    due_at                  TIMESTAMPTZ         NOT NULL,
    allow_late              BOOLEAN             NOT NULL DEFAULT    TRUE,
    late_penalty_percent    INTEGER             NOT NULL DEFAULT    0,
    status                  TEXT                NOT NULL DEFAULT    'draft',
    published_at            TIMESTAMPTZ,
    submission_count        INTEGER             NOT NULL DEFAULT    0,
    created_at              TIMESTAMPTZ         NOT NULL DEFAULT    NOW(),
    updated_at              TIMESTAMPTZ         NOT NULL DEFAULT    NOW(),
    deleted_at              TIMESTAMPTZ,

    CHECK(char_length(title) BETWEEN 2 AND 200),
    CHECK(late_penalty_percent BETWEEN 0 AND 100),
    CHECK(status IN ('draft', 'published','closed')),
    CHECK(max_score > 0)
);
CREATE INDEX idx_assignments_channel    ON assignments(channel_id, status, due_at) WHERE deleted_at IS NULL;
CREATE INDEX idx_assignments_workspace  ON assignments(workspace_id, status, due_at) WHERE deleted_at IS NULL;
-- notification job for assignments due soon
CREATE INDEX idx_assignments_due_soon   ON assignments(due_at) WHERE status = 'published' AND deleted_at IS NULL;

CREATE TRIGGER set_updated_at_assignments
BEFORE UPDATE ON assignments
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();


CREATE TABLE assignment_submissions(
    id                      UUID    PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    assignment_id           UUID                NOT NULL REFERENCES assignments(id)      ON DELETE CASCADE,
    student_user_id         UUID                NOT NULL REFERENCES users(id)            ON DELETE RESTRICT,
    content                 TEXT                NOT NULL DEFAULT    '',
    attachments             JSONB               NOT NULL DEFAULT    '[]'::jsonb,
    submitted_at            TIMESTAMPTZ         NOT NULL DEFAULT    NOW(),
    is_late                 BOOLEAN             NOT NULL DEFAULT    FALSE,
    score                   NUMERIC(6,2),
    max_score               INTEGER             NOT NULL,
    grader_user_id          UUID                REFERENCES users(id)                      ON DELETE SET NULL,
    graded_at               TIMESTAMPTZ,
    feedback                TEXT,
    created_at              TIMESTAMPTZ         NOT NULL DEFAULT    NOW(),
    updated_at              TIMESTAMPTZ         NOT NULL DEFAULT    NOW(),

    UNIQUE(assignment_id, student_user_id)
);
CREATE INDEX idx_submission_assignment ON assignment_submissions(assignment_id, student_user_id);
CREATE INDEX idx_submission_student    ON assignment_submissions(student_user_id, submitted_at DESC);
CREATE INDEX idx_submission_ungraded   ON assignment_submissions(assignment_id) WHERE graded_at IS NULL;

CREATE TRIGGER set_updated_at_assignment_submissions
BEFORE UPDATE ON assignment_submissions
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();