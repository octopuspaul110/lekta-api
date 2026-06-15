-- Add migration script here

CREATE TABLE users(
    id                  UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    email               VARCHAR(255) NOT NULL,
    email_verified      BOOLEAN      NOT NULL DEFAULT FALSE,
    password_hash       VARCHAR(255),
    full_name           VARCHAR(100) NOT NULL,
    phone               TEXT,
    -- This is the avatar key stored in aws s3(for now), nullable
    avatar_key          TEXT,
    google_sub          TEXT,
    is_platform_admin   BOOLEAN     NOT NULL DEFAULT FALSE,
    timezone            TEXT        NOT NULL DEFAULT 'Africa/Lagos',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at       TIMESTAMPTZ,   
    deleted_at          TIMESTAMPTZ,

    CHECK(char_length(full_name) >= 2 AND char_length(full_name) <= 100)      
);

CREATE UNIQUE INDEX idx_users_email_lower    ON users(lower(email)) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX idx_users_google_sub     ON users(google_sub)   WHERE google_sub IS NOT NULL AND deleted_at IS NULL;
CREATE        INDEX idx_users_created_at     ON users(created_at DESC);
CREATE        INDEX idx_users_platform_admin ON users(is_platform_admin) WHERE is_platform_admin = TRUE;

