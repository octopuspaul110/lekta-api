-- Add migration script here

CREATE TABLE notifications(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    user_id             UUID             NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id        UUID                      REFERENCES workspaces(id) ON DELETE CASCADE,
    type                TEXT             NOT NULL,
    title               TEXT             NOT NULL,
    body                TEXT             NOT NULL,
    deep_link           TEXT,
    metadata            JSONB            NOT NULL DEFAULT '{}'::jsonb,
    is_read             BOOLEAN          NOT NULL DEFAULT FALSE,
    read_at             TIMESTAMPTZ,
    fcm_sent            BOOLEAN          NOT NULL DEFAULT FALSE,
    fcm_sent_at         TIMESTAMPTZ,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_notifications_user_unread ON notifications(user_id, is_read, created_at DESC) WHERE is_read = FALSE;
CREATE INDEX idx_notifications_user_all    ON notifications(user_id, created_at DESC);

CREATE TABLE jobs(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    job_type            TEXT             NOT NULL,
    payload             JSONB            NOT NULL DEFAULT '{}'::jsonb,
    priority            INTEGER          NOT NULL DEFAULT 0,
    status              TEXT             NOT NULL DEFAULT 'pending',
    attempts            INTEGER          NOT NULL DEFAULT 0,
    max_attempts        INTEGER          NOT NULL DEFAULT 3,
    run_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    started_at          TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    locked_by           TEXT,
    last_error          TEXT,
    error_history       JSONB            NOT NULL DEFAULT '[]'::jsonb,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(status IN ('pending', 'processing', 'completed', 'failed', 'dead'))
);
CREATE INDEX idx_jobs_dequeue           ON jobs(priority DESC, run_at ASC) WHERE status = 'pending';
CREATE INDEX idx_jobs_dead              ON jobs(job_type, completed_at DESC) WHERE status = 'dead';
-- for monitoring
CREATE INDEX idx_jobs_type              ON jobs(job_type, status);

CREATE TABLE audit_log(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    actor_user_id       UUID                      REFERENCES users(id) ON DELETE SET NULL,
    workspace_id        UUID                      REFERENCES workspaces(id) ON DELETE SET NULL,
    action              TEXT             NOT NULL,
    entity_type         TEXT             NOT NULL,
    entity_id           UUID,
    old_values          JSONB,
    new_values          JSONB,
    ip_address          INET,
    user_agent          TEXT,
    created_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_audit_workspace         ON audit_log(workspace_id, created_at DESC);
CREATE INDEX idx_audit_actor             ON audit_log(actor_user_id, created_at DESC);
CREATE INDEX idx_audit_entity            ON audit_log(entity_type, entity_id, created_at DESC);

CREATE TABLE scheduled_runs(
    schedule_name       TEXT             NOT NULL,
    scheduled_for       TIMESTAMPTZ      NOT NULL,
    started_at          TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ,
    
    PRIMARY KEY (schedule_name, scheduled_for)
);