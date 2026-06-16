-- Add migration script here

CREATE TABLE channels(
    id                      UUID    PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    workspace_id            UUID                NOT NULL REFERENCES workspaces(id)       ON DELETE CASCADE,
    name                    TEXT                NOT NULL,
    display_name            TEXT                NOT NULL,
    description             TEXT,
    channel_type            TEXT                NOT NULL,
    visibility              TEXT                NOT NULL DEFAULT 'public',
    post_permission         TEXT                NOT NULL DEFAULT 'everyone',
    created_by_user_id       UUID                NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    is_default              BOOLEAN             NOT NULL DEFAULT FALSE,
    archived                BOOLEAN             NOT NULL DEFAULT FALSE,
    archived_at             TIMESTAMPTZ,
    message_count           INTEGER             NOT NULL DEFAULT 0,
    last_message_at         TIMESTAMPTZ,
    created_at              TIMESTAMPTZ         NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ         NOT NULL DEFAULT NOW(),   

    CHECK(name ~ '^[a-z0-9][a-z0-9-]{0,30}[a-z0-9]$'),
    CHECK(channel_type      IN ('subject', 'announcement', 'general')),
    CHECK(visibility        IN ('public', 'private')),
    CHECK(post_permission   IN ('everyone', 'tutors_and_admins', 'admins_only')),
    UNIQUE(workspace_id, name)
);
CREATE INDEX idx_channels_workspace             ON channels(workspace_id, archived, channel_type);
CREATE INDEX idx_channels_workspace_activity    ON channels(workspace_id,last_message_at DESC NULLS LAST) WHERE archived = FALSE;

CREATE TABLE channel_members(
    id                      UUID     PRIMARY KEY  NOT NULL DEFAULT gen_random_uuid(),
    channel_id              UUID                  NOT NULL REFERENCES channels(id)      ON DELETE CASCADE,
    user_id                 UUID                  NOT NULL REFERENCES users(id)         ON DELETE CASCADE,
    joined_at               TIMESTAMPTZ           NOT NULL DEFAULT NOW(),
    -- pointer to the last message the user has seen
    last_read_message_id    UUID,
    last_read_at            TIMESTAMPTZ,
    notification_muted      BOOLEAN NOT NULL DEFAULT FALSE,
    muted_until             TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(channel_id, user_id)              
);
CREATE INDEX idx_channel_members_user ON channel_members(user_id, channel_id);

CREATE TABLE direct_messages(
    id                        UUID      PRIMARY KEY NOT NULL DEFAULT        gen_random_uuid(),
    workspace_id              UUID                  NOT NULL REFERENCES     workspaces(id)      ON DELETE CASCADE,
    is_group                  BOOLEAN               NOT NULL DEFAULT        FALSE,
    last_message_at           TIMESTAMPTZ,
    created_at                TIMESTAMPTZ           NOT NULL DEFAULT        NOW()
);

CREATE TABLE dm_participants(
    id                        UUID          PRIMARY KEY          DEFAULT        gen_random_uuid(),
    dm_id                     UUID                      NOT NULL REFERENCES     direct_messages(id) ON DELETE CASCADE,
    user_id                   UUID                      NOT NULL REFERENCES     users(id)           ON DELETE CASCADE,
    last_read_message_id      UUID, 
    last_read_at              TIMESTAMPTZ,  
    joined_at                 TIMESTAMPTZ               NOT NULL DEFAULT        NOW(),
    left_at                   TIMESTAMPTZ,

    UNIQUE(dm_id,user_id)
);
CREATE INDEX idx_dm_participants_user   ON dm_participants(user_id, dm_id)  WHERE left_at IS NULL;

CREATE TABLE messages(
    id                      UUID                        PRIMARY KEY     NOT NULL                DEFAULT   gen_random_uuid(),
    channel_id              UUID                        REFERENCES      channels(id)            ON DELETE CASCADE,
    dm_id                   UUID                        REFERENCES      direct_messages(id)     ON DELETE CASCADE,
    workspace_id            UUID        NOT NULL        REFERENCES      workspaces(id)          ON DELETE CASCADE,
    sender_user_id          UUID        NOT NULL        REFERENCES      users(id)               ON DELETE RESTRICT,
    content                 TEXT        NOT NULL        DEFAULT         '',
    -- array of s3_key, filename, content_type,     size_bytes
    attachments             JSONB       NOT NULL        DEFAULT         '[]'::jsonb,
    thread_parent_id        UUID        REFERENCES      messages(id)                            ON DELETE CASCADE,
    thread_reply_count      INTEGER     NOT NULL        DEFAULT         0,
    edited                  BOOLEAN     NOT NULL        DEFAULT         FALSE,
    edited_at               TIMESTAMPTZ,
    deleted                 BOOLEAN     NOT NULL        DEFAULT         FALSE,
    deleted_at              TIMESTAMPTZ,
    deleted_by_user_id      UUID        REFERENCES      users(id)                                ON DELETE CASCADE,
    tsv                     tsvector    GENERATED       ALWAYS          AS                      (to_tsvector('english', content)) STORED,
    created_at              TIMESTAMPTZ NOT NULL        DEFAULT         NOW(),

    CHECK((channel_id IS NOT NULL AND dm_id IS NULL) OR (channel_id IS NULL AND dm_id IS NOT NULL))
);

CREATE INDEX idx_messages_channel   ON messages(channel_id,created_at DESC)       WHERE channel_id        IS NOT NULL AND deleted = FALSE;    
CREATE INDEX idx_messages_dm        ON messages(dm_id, created_at DESC)           WHERE dm_id             IS NOT NULL AND deleted = FALSE;    
CREATE INDEX idx_messages_thread    ON messages(thread_parent_id,created_at ASC)  WHERE thread_parent_id  IS NOT NULL;
CREATE INDEX idx_messages_sender    ON messages(sender_user_id, created_at  DESC);
CREATE INDEX idx_messages_tsv       ON messages USING GIN (tsv);
CREATE INDEX idx_messages_workspace ON messages(workspace_id, created_at DESC)    WHERE deleted = FALSE;

CREATE TABLE message_reactions(
    id                        UUID      PRIMARY KEY NOT NULL DEFAULT        gen_random_uuid(),
    message_id                UUID                  NOT NULL REFERENCES     messages(id) ON DELETE CASCADE,
    user_id                   UUID                  NOT NULL REFERENCES     users(id)    ON DELETE CASCADE,
    emoji                     TEXT                  NOT NULL,
    created_at                TIMESTAMPTZ           NOT NULL DEFAULT        NOW(),  

    CHECK(char_length(emoji) >= 1 AND char_length(emoji) <= 32),
    UNIQUE(message_id, user_id, emoji)
);

CREATE TRIGGER set_updated_at_channels
BEFORE UPDATE ON channels
FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();