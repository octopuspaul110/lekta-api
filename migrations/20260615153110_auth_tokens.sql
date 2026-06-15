-- Add migration script here

CREATE TABLE refresh_tokens(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    user_id                 UUID             NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash              TEXT             NOT NULL,
    device_name             TEXT,
    ip_address              INET,
    user_agent              TEXT,
    expires_at              TIMESTAMPTZ      NOT NULL,
    revoked                 BOOLEAN          NOT NULL DEFAULT FALSE,
    revoked_at              TIMESTAMPTZ,
    replaced_by_token_id    UUID REFERENCES refresh_tokens(id),
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_refresh_tokens_hash     ON refresh_tokens(token_hash);
CREATE        INDEX idx_refresh_tokens_user_id  ON refresh_tokens(user_id, revoked, expires_at) WHERE revoked = FALSE;
CREATE        INDEX idx_refresh_tokens_expiry   ON refresh_tokens(expires_at) WHERE revoked = FALSE;

CREATE TABLE password_reset_tokens(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT       gen_random_uuid(),
    user_id                UUID             NOT NULL REFERENCES    users(id)   ON DELETE CASCADE,
    token_hash              TEXT             NOT NULL,
    expires_at              TIMESTAMPTZ      NOT NULL,
    used                    BOOLEAN          NOT NULL DEFAULT FALSE,
    used_at                 TIMESTAMPTZ,
    ip_address              INET,
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW()      
);
CREATE UNIQUE INDEX idx_password_reset_tokens_hash ON password_reset_tokens(token_hash);
CREATE        INDEX idx_password_reset_user_id     ON password_reset_tokens(user_id, used, expires_at);

CREATE TABLE email_verification_tokens(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT       gen_random_uuid(),
    user_id                UUID             NOT NULL REFERENCES    users(id)   ON DELETE CASCADE,
    token_hash              TEXT             NOT NULL,
    expires_at              TIMESTAMPTZ      NOT NULL,
    used                    BOOLEAN          NOT NULL DEFAULT FALSE,
    used_at                 TIMESTAMPTZ,
    ip_address              INET,
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW()      
);
CREATE UNIQUE INDEX idx_email_verification_tokens_hash      ON email_verification_tokens(token_hash);
CREATE        INDEX idx_email_verification_tokens_user_id   ON email_verification_tokens(user_id, used);

CREATE TABLE device_tokens(
    id                      UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    user_id                 UUID             NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    fcm_token               TEXT             NOT NULL,
    platform                TEXT             NOT NULL,
    device_name             TEXT,
    device_model            TEXT,
    os_version              TEXT,
    app_version             TEXT,
    is_active               BOOLEAN          NOT NULL DEFAULT TRUE,
    last_seen_at            TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    created_at              TIMESTAMPTZ      NOT NULL DEFAULT NOW()

    CHECK(platform IN ('ios','android','web'))
);
CREATE UNIQUE INDEX idx_device_tokens_fcm_token     ON device_tokens(fcm_token);
CREATE        INDEX idx_device_tokens_user_active   ON device_tokens(user_id,is_active) WHERE is_active = TRUE;
CREATE        INDEX idx_device_tokens_stale         ON device_tokens(last_seen_at)      WHERE is_active = TRUE;