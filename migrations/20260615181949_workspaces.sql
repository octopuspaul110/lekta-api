-- Add migration script here
CREATE TABLE workspaces(
    id                              UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    name                            TEXT NOT NULL,
    slug                            TEXT NOT NULL,
    description                     TEXT,
    -- avatar key stored             in aws s3 (for now)
    avatar_key                      TEXT,
    cover_image_key                 TEXT,
    -- A center can have             {jamb, waec, neco} or {ielts, toefl} or just {undergraduate}
    focus_areas                     TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    proprietor_user_id              UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    payment_mode                    TEXT NOT NULL DEFAULT 'lekta_managed',
    -- NULL for external payment
    paystack_subaccount_code        TEXT,
    paystack_subaccount_status      TEXT NOT NULL DEFAULT 'pending' ,
    platform_fee_basis_points       INTEGER NOT NULL DEFAULT 150,
    subscription_status             TEXT NOT NULL DEFAULT 'trial',
    subscription_tier               TEXT,
    monthly_subscription_kobo       BIGINT,
    trial_ends_at                   TIMESTAMPTZ,
    -- Fixable workspace configuration
    settings                        JSONB NOT NULL DEFAULT '{}'::jsonb,
    student_count                   INTEGER NOT NULL DEFAULT 0,
    tutor_count                     INTEGER NOT NULL DEFAULT 0,
    created_at                      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- soft delete
    deleted_at                      TIMESTAMPTZ,

    CHECK(char_length(name) >= 2 AND char_length(name) <= 100),
    CHECK(slug ~ '^[a-z0-9][a-z0-9-]{1,50}[a-z0-9]$'),
    CHECK(array_length(focus_areas, 1) >= 1),
    CHECK(payment_mode IN ('lekta_managed','external','hybrid')),
    CHECK(paystack_subaccount_status IN ('pending', 'active', 'suspended', 'not_applicable')),
    CHECK(platform_fee_basis_points >= 0 AND platform_fee_basis_points <= 10000),
    CHECK(subscription_status IN ('trial','active','past_due','cancelled')),
    CHECK(subscription_tier IN ('free','starter','growth','scale'))
);

CREATE        INDEX idx_workspaces_focus_areas ON workspaces                        USING GIN (focus_areas);
CREATE UNIQUE INDEX idx_workspaces_slug        ON workspaces(slug)                  WHERE deleted_at IS NULL;
CREATE        INDEX idx_workspaces_proprietor  ON workspaces(proprietor_user_id)    WHERE deleted_at IS NULL;
CREATE        INDEX idx_workspaces_created_at  ON workspaces(created_at DESC);

CREATE TRIGGER      set_updated_at_workspaces
BEFORE UPDATE ON workspaces
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE workspace_slug_redirects(
    old_slug        TEXT PRIMARY KEY,
    workspace_id    UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    changed_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE workspace_members(
    id                          UUID PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    workspace_id                UUID             NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id                     UUID             NOT NULL REFERENCES users(id)     ON DELETE CASCADE,
    role                        TEXT             NOT NULL,
    status                      TEXT             NOT NULL DEFAULT    'active',
    invited_by_user_id          UUID                      REFERENCES users(id) ON DELETE SET NULL,
    joined_at                   TIMESTAMPTZ      NOT NULL DEFAULT    NOW(),
    removed_at                  TIMESTAMPTZ,
    onboarded_at                TIMESTAMPTZ               DEFAULT    NULL,
    notification_preferences    JSONB            NOT NULL DEFAULT    '{}'::jsonb,
    created_at                  TIMESTAMPTZ      NOT NULL DEFAULT    NOW(),
    updated_at                  TIMESTAMPTZ      NOT NULL DEFAULT    NOW(),

    UNIQUE(workspace_id,user_id),
    CHECK(role IN ('proprietor', 'admin', 'tutor', 'student')),
    CHECK(status IN ('active', 'invited', 'suspended', 'removed'))
);
CREATE UNIQUE INDEX idx_workspace_one_proprietor ON workspace_members(workspace_id)      WHERE role = 'proprietor' AND status = 'active';
CREATE        INDEX idx_workspace_members_user   ON workspace_members(user_id, status)   WHERE status = 'active';
CREATE        INDEX idx_workspace_members_role   ON workspace_members(workspace_id, role, status);

CREATE TRIGGER set_updated_at_workspace_members
BEFORE UPDATE ON workspace_members
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE workspace_invitations(
    id                          UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    workspace_id                UUID             NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email                       TEXT             NOT NULL,
    role                        TEXT             NOT NULL,  
    token_hash                  TEXT             NOT NULL,
    invited_by_user_id          UUID             NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    expires_at                  TIMESTAMPTZ      NOT NULL DEFAULT NOW() + INTERVAL '7 days',
    accepted                    BOOLEAN          NOT NULL DEFAULT FALSE,  
    accepted_at                 TIMESTAMPTZ,
    accepted_by_user_id         UUID                      REFERENCES users(id) ON DELETE SET NULL,
    personal_message            TEXT,
    created_at                  TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    CHECK(role IN ('admin','tutor','student'))
);
CREATE UNIQUE INDEX idx_workspace_invitations_token             ON workspace_invitations(token_hash);
CREATE        INDEX idx_workspace_invitations_workspace_email   ON workspace_invitations(workspace_id, email, accepted);


