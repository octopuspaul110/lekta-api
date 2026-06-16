-- Add migration script here
CREATE TABLE tuition_plans(
    id                  UUID        PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    workspace_id        UUID                    NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name                TEXT                    NOT NULL,
    description         TEXT,       
    amount_kobo         BIGINT                  NOT NULL,
    duration_days       INTEGER                 NOT NULL,
    is_active           BOOLEAN                 NOT NULL DEFAULT TRUE,
    created_by_user_id  UUID                    NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    enrollment_count    INTEGER                 NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    deleted_at          TIMESTAMPTZ,

    CHECK(char_length(name) BETWEEN 2 AND 100),
    CHECK(amount_kobo >= 0),
    CHECK(duration_days > 0)
);
CREATE INDEX idx_tuition_plans_workspace ON tuition_plans(workspace_id, is_active) WHERE deleted_at IS NULL;

CREATE TRIGGER set_updated_at_tuition_plans
BEFORE UPDATE ON tuition_plans
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE payments(
    id                  UUID        PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    reference           TEXT                    NOT NULL UNIQUE,
    workspace_id        UUID                    NOT NULL REFERENCES workspaces(id)  ON DELETE CASCADE,
    payer_user_id       UUID                    NOT NULL REFERENCES users(id)       ON DELETE RESTRICT,
    -- payment for an enrollment
    enrollment_id       UUID,
    -- for AI feature
    ai_subscription_id  UUID,
    payment_purpose     TEXT                    NOT NULL,
    amount_kobo         BIGINT                  NOT NULL,
    platform_fee_kobo   BIGINT                  NOT NULL DEFAULT 0,
    center_amount_kobo  BIGINT                  NOT NULL DEFAULT 0,
    paystack_fee_kobo   BIGINT                  NOT NULL DEFAULT 0,
    status              TEXT                    NOT NULL DEFAULT 'pending',
    paystack_response   JSONB,
    failure_reason      TEXT,
    paid_at             TIMESTAMPTZ,
    created_at          TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ             NOT NULL DEFAULT NOW(),

    CHECK(status IN ('pending', 'successful', 'failed', 'abandoned', 'reversed')),
    CHECK (payment_purpose IN ('tuition', 'ai_subscription', 'workspace_subscription'))
);
CREATE UNIQUE INDEX idx_payments_reference              ON payments(reference);
CREATE INDEX        idx_payments_workspace_status_date  ON payments(workspace_id, status, created_at DESC);
CREATE INDEX        idx_payments_payer                  ON payments(payer_user_id, created_at DESC);
CREATE INDEX        idx_payments_pending_old            ON payments(created_at) WHERE status = 'pending';

CREATE TRIGGER set_updated_at_payments
BEFORE UPDATE ON payments
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE enrollments(
    id                      UUID        PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    workspace_id            UUID                    NOT NULL REFERENCES workspaces(id)      ON DELETE CASCADE,
    student_user_id         UUID                    NOT NULL REFERENCES users(id)           ON DELETE RESTRICT,
    tuition_plan_id         UUID                    NOT NULL REFERENCES tuition_plans(id)   ON DELETE RESTRICT,
    status                  TEXT                    NOT NULL DEFAULT    'pending',
    enrollment_source       TEXT                    NOT NULL DEFAULT    'paystack',
    -- centers own payment reference for manual enrollments
    enrollment_reference    TEXT,
    payment_id              UUID                             REFERENCES payments(id) ON DELETE SET NULL,
    starts_at               TIMESTAMPTZ,
    ends_at                 TIMESTAMPTZ,
    created_at              TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ             NOT NULL DEFAULT NOW(),

    CHECK(status IN ('pending', 'active', 'expired', 'cancelled', 'refunded')),
    -- manual for enrollments created by admins of external payment workspaces
    CHECK(enrollment_source IN ('paystack', 'manual'))
);

CREATE INDEX idx_enrollments_student            ON enrollments(student_user_id, status);
CREATE INDEX idx_enrollments_workspace_status   ON enrollments(workspace_id, status, ends_at);
-- for daily expiry jobs
CREATE INDEX idx_enrollments_expiry             ON enrollments(ends_at) WHERE status = 'active'; 


--idempotency log for paystack webhooks deliveries
CREATE TABLE paystack_webhook_events(
    event_id            TEXT        PRIMARY KEY,
    event_type          TEXT                    NOT NULL,
    received_at         TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    processed           BOOLEAN                 NOT NULL DEFAULT FALSE,
    processed_at        TIMESTAMPTZ,
    raw_payload         JSONB                   NOT NULL,
    -- populated on failed processing
    error_message       TEXT    
);
CREATE INDEX idx_paystack_webhook_unprocessed ON paystack_webhook_events(received_at) WHERE processed = FALSE;

CREATE TRIGGER set_updated_at_enrollments
BEFORE UPDATE ON enrollments
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();