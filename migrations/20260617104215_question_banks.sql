-- Add migration script here
CREATE TABLE question_banks(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    owner_type          TEXT             NOT NULL,
    owner_workspace_id  UUID                      REFERENCES workspaces(id) ON DELETE CASCADE,
    name                TEXT             NOT NULL,
    description         TEXT,
    subject             TEXT             NOT NULL,
    exam_type           TEXT             NOT NULL,
    language            TEXT             NOT NULL DEFAULT 'en',
    -- workspace banks are draft until published
    is_published        BOOLEAN          NOT NULL DEFAULT FALSE,
    created_by_user_id  UUID             NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    question_count      INTEGER          NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(owner_type IN ('workspace', 'platform')),
    CHECK(char_length(name) BETWEEN 2 AND 100),
    CHECK(exam_type IN ('jamb', 'waec', 'neco', 'ielts', 'toefl', 'gre', 'undergraduate', 'custom')),
    CHECK((owner_type = 'platform' AND owner_workspace_id IS NULL) OR (owner_type = 'workspace' AND owner_workspace_id IS NOT NULL))
);

CREATE INDEX idx_question_bank_owner ON question_banks(owner_type, owner_workspace_id, is_published);
CREATE INDEX idx_question_bank_subject ON question_banks(subject, exam_type);

CREATE TRIGGER set_updated_at_question_bank
BEFORE UPDATE ON question_banks
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE questions(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    question_bank_id    UUID             NOT NULL REFERENCES question_banks(id) ON DELETE CASCADE,
    question_text       TEXT             NOT NULL,
    question_type       TEXT             NOT NULL,
    options             JSONB,
    correct_answers     JSONB            NOT NULL,
    explanation         TEXT,
    difficulty          TEXT             NOT NULL DEFAULT 'medium',
    topic_tags          TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    year                INTEGER,
    marks               INTEGER          NOT NULL DEFAULT 1,
    -- media s3 keys
    media_keys          JSONB            NOT NULL DEFAULT '[]'::jsonb,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    deleted_at          TIMESTAMPTZ,


    CHECK(question_type IN ('multiple_choice_single', 'multiple_choice_multi', 'true_false', 'short_answer', 'essay','numeric','fill_in_the_blanks')),
    CHECK(difficulty IN ('easy', 'medium', 'hard')),
    CHECK(marks > 0)
);
CREATE INDEX idx_questions_bank ON questions(question_bank_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_questions_tags ON questions USING GIN(topic_tags);
CREATE INDEX idx_questions_bank_difficulty_year ON questions(question_bank_id, difficulty, year) WHERE deleted_at IS NULL;

CREATE TRIGGER set_updated_at_questions
BEFORE UPDATE ON questions
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE exams(
    id                          UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    workspace_id                UUID             NOT NULL REFERENCES workspaces(id)     ON DELETE CASCADE,
    creator_user_id             UUID             NOT NULL REFERENCES users(id)          ON DELETE RESTRICT,
    title                       TEXT             NOT NULL,
    description                 TEXT,
    selection_criteria          JSONB            NOT NULL,
    duration_minutes            INTEGER          NOT NULL DEFAULT 60,
    total_marks                 INTEGER          NOT NULL DEFAULT 0,
    pass_mark_percent           INTEGER          NOT NULL DEFAULT 50,
    scheduled_start_at          TIMESTAMPTZ,
    scheduled_ends_at           TIMESTAMPTZ,
    eligibility                 JSONB            NOT NULL,
    allow_retakes               BOOLEAN          NOT NULL DEFAULT FALSE,
    max_attempts                INTEGER          NOT NULL DEFAULT 1,
    randomize_questions         BOOLEAN          NOT NULL DEFAULT TRUE,
    randomize_options           BOOLEAN          NOT NULL DEFAULT TRUE,
    show_results_immediately    BOOLEAN          NOT NULL DEFAULT FALSE,
    status                      TEXT             NOT NULL DEFAULT 'draft',
    attempt_count               INTEGER          NOT NULL DEFAULT 0,
    created_at                  TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at                  TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    deleted_at                  TIMESTAMPTZ,

    CHECK(duration_minutes > 0),
    CHECK(total_marks >= 0),
    CHECK(pass_mark_percent BETWEEN 0 AND 100),
    CHECK(status IN ('draft', 'scheduled', 'ongoing', 'completed', 'archived'))
);
CREATE INDEX idx_exams_workspace_status ON exams(workspace_id, status, scheduled_start_at) WHERE deleted_at IS NULL;

CREATE TRIGGER set_updated_at_exams
BEFORE UPDATE ON exams
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE exam_attempts(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    exam_id             UUID             NOT NULL REFERENCES exams(id) ON DELETE CASCADE,
    student_user_id     UUID             NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    attempt_number      INTEGER          NOT NULL DEFAULT 1,
    started_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    deadline_at         TIMESTAMPTZ      NOT NULL,
    submitted_at        TIMESTAMPTZ,
    auto_submitted      BOOLEAN          NOT NULL DEFAULT FALSE,
    total_score         NUMERIC(8,2),
    percent_score       NUMERIC(5,2),
    pass_status         TEXT             NOT NULL DEFAULT 'pending',
    status              TEXT             NOT NULL DEFAULT 'in_progress',
    graded_at           TIMESTAMPTZ,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(attempt_number > 0),
    CHECK(pass_status   IN ('pending', 'passed', 'failed')),
    CHECK(status        IN ('in_progress', 'submitted', 'graded', 'voided')),
    UNIQUE(exam_id, student_user_id, attempt_number)
);
CREATE INDEX idx_exam_attempts_exam_student     ON exam_attempts(exam_id, student_user_id);
CREATE INDEX idx_exam_attempts_detail           ON exam_attempts(deadline_at)                   WHERE status = 'in_progress';
CREATE INDEX idx_exam_attempts_pending_grading  ON exam_attempts(exam_id)                       WHERE status = 'submitted';

CREATE TRIGGER set_updated_at_exam_attempts
BEFORE UPDATE ON exam_attempts
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE exam_questions(
    id                                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    exam_attempt_id                     UUID             NOT NULL REFERENCES exam_attempts(id) ON DELETE CASCADE,
    question_id                         UUID             NOT NULL REFERENCES questions(id) ON DELETE RESTRICT,
    question_order                      INTEGER          NOT NULL,
    shuffled_options                    JSONB,
    created_at                          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at                          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(question_order > 0),
    UNIQUE(exam_attempt_id, question_id)
);
CREATE INDEX idx_exam_questions_attempt ON exam_questions(exam_attempt_id, question_order);

CREATE TRIGGER set_updated_at_exam_questions
BEFORE UPDATE ON exam_questions
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE exam_answers(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    exam_attempt_id     UUID             NOT NULL REFERENCES exam_attempts(id) ON DELETE CASCADE,
    exam_question_id    UUID             NOT NULL REFERENCES exam_questions(id) ON DELETE RESTRICT,
    student_answer      JSONB,
    is_correct          BOOLEAN,
    marks_awarded       NUMERIC(6,2),
    time_spent_seconds  INTEGER,
    grader_user_id      UUID                      REFERENCES users(id) ON DELETE SET NULL,
    grader_feedback     TEXT,
    graded_at           TIMESTAMPTZ,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    UNIQUE(exam_attempt_id,exam_question_id)
);
CREATE INDEX idx_exam_answers_attempt ON exam_answers(exam_attempt_id);

CREATE TRIGGER set_updated_at_exam_answers
BEFORE UPDATE ON exam_answers
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();