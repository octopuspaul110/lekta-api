-- Add migration script here
CREATE TABLE ai_feature_enablement(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    workspace_id            UUID             NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    feature                 TEXT             NOT NULL,
    is_enabled              BOOLEAN          NOT NULL DEFAULT FALSE,
    price_kobo              BIGINT           NOT NULL DEFAULT 0,
    duration_days           INTEGER          NOT NULL DEFAULT 0,
    monthly_usage_limit     INTEGER          NOT NULL DEFAULT 100,
    -- if true, every active enrollee will get access to this feature without purchase
    included_with_tuition   BOOLEAN          NOT NULL DEFAULT FALSE,
    created_by_user_id      UUID             NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(feature IN ('note_summaries', 'ai_tutor_chat', 'exam_analytics')),
    UNIQUE(workspace_id, feature)
);

CREATE TRIGGER set_updated_at_ai_feature_enablement
BEFORE UPDATE ON ai_feature_enablement
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE ai_subscriptions(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    workspace_id            UUID             NOT NULL REFERENCES workspaces(id)     ON DELETE CASCADE,
    student_user_id         UUID             NOT NULL REFERENCES users(id)          ON DELETE RESTRICT,
    feature                 TEXT             NOT NULL,
    started_at              TIMESTAMPTZ      NOT NULL,
    expires_at              TIMESTAMPTZ      NOT NULL,
    payment_id              UUID             REFERENCES payments(id) ON DELETE SET NULL,
    usage_count             INTEGER          NOT NULL DEFAULT 0,
    monthly_usage_count     INTEGER          NOT NULL DEFAULT 0,
    monthly_usage_reset_at  TIMESTAMPTZ      NOT NULL DEFAULT date_trunc('month', NOW()) + INTERVAL '1 month',
    status                  TEXT             NOT NULL DEFAULT 'active',
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(feature IN ('note_summaries', 'ai_tutor_chat', 'exam_analytics')),
    CHECK(status IN ('active', 'expired', 'cancelled'))
);
CREATE INDEX idx_ai_subs_student_active    ON ai_subscriptions(student_user_id, feature, status, expires_at);

CREATE TRIGGER set_updated_at_ai_subscriptions
BEFORE UPDATE ON ai_subscriptions
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE notes(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    owner_user_id           UUID             NOT NULL REFERENCES users(id)      ON DELETE CASCADE,
    workspace_id            UUID                      REFERENCES workspaces(id) ON DELETE SET NULL,
    title                   TEXT             NOT NULL,
    content                 TEXT             NOT NULL DEFAULT '',
    -- sha 256 of content, changes invalidate cached summary
    content_hash            TEXT             NOT NULL,
    tags                    TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    is_pinned               BOOLEAN          NOT NULL DEFAULT FALSE,
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    deleted_at              TIMESTAMPTZ,

    CHECK(char_length(title) BETWEEN 1 AND 200)
);
CREATE INDEX idx_notes_owner    ON notes(owner_user_id, updated_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_notes_tags     ON notes USING GIN (tags)                WHERE deleted_at IS NULL;

CREATE TRIGGER set_updated_at_notes
BEFORE UPDATE ON notes
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE note_summaries(
    note_id           UUID PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    summary           TEXT             NOT NULL,
    key_concepts      TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    questions_to_review TEXT[]         NOT NULL DEFAULT ARRAY[]::TEXT[],
    content_hash      TEXT             NOT NULL,
    generated_at      TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    model_version     TEXT             NOT NULL
);

CREATE TABLE ai_chat_sessions(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    student_user_id         UUID             NOT NULL REFERENCES users(id)      ON DELETE CASCADE,
    workspace_id            UUID             NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title                   TEXT             NOT NULL DEFAULT 'New chat',
    context_note_ids        UUID[]           NOT NULL DEFAULT ARRAY[]::UUID[],
    is_archived             BOOLEAN          NOT NULL DEFAULT FALSE,
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_ai_chat_sessions_student    ON ai_chat_sessions(student_user_id, workspace_id, updated_at DESC) WHERE is_archived = FALSE;

CREATE TRIGGER set_updated_at_ai_chat_sessions
BEFORE UPDATE ON ai_chat_sessions
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE ai_chat_messages(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    session_id              UUID             NOT NULL REFERENCES ai_chat_sessions(id) ON DELETE CASCADE,
    role                    TEXT             NOT NULL,
    content                 TEXT             NOT NULL,
    input_tokens            INTEGER,
    output_tokens           INTEGER,
    model_version           TEXT,
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(role IN ('user', 'assistant'))
);
CREATE INDEX idx_ai_chat_messages_session    ON ai_chat_messages(session_id, created_at ASC);


CREATE TABLE exam_analytics(
    exam_attempt_id         UUID PRIMARY KEY          REFERENCES exam_attempts(id)  ON DELETE CASCADE,
    overall_assessment      TEXT             NOT NULL,
    strengths               TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    weaknesses              TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    topic_breakdown         JSONB            NOT NULL DEFAULT '{}'::jsonb,
    study_recommendations   TEXT[]           NOT NULL DEFAULT ARRAY[]::TEXT[],
    generated_at            TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    model_version           TEXT             NOT NULL
);
