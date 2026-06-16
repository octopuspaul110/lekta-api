-- Add migration script here
CREATE TABLE classes(
    id                      UUID        PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    workspace_id            UUID                    NOT NULL REFERENCES workspaces(id)  ON DELETE CASCADE,
    channel_id              UUID                             REFERENCES channels(id)    ON DELETE SET NULL,
    tutor_user_id           UUID                    NOT NULL REFERENCES users(id)       ON DELETE RESTRICT,
    title                   TEXT                    NOT NULL,
    description             TEXT,       
    -- physical location    
    location                TEXT                    NOT NULL,  
    starts_at               TIMESTAMPTZ             NOT NULL,  
    ends_at                 TIMESTAMPTZ             NOT NULL,   
    -- RFC 5545 RRULE for recurring classes
    recurrence_rule         TEXT,   
    recurrence_parent_id    UUID                             REFERENCES classes(id)     ON DELETE CASCADE,   
    status                  TEXT                    NOT NULL DEFAULT    'scheduled',
    cancellation_reason     TEXT,
    self_checkin_enabled    BOOLEAN                 NOT NULL DEFAULT    FALSE,
    created_by_user_id      UUID                    NOT NULL REFERENCES users(id)       ON DELETE RESTRICT,
    created_at              TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ             NOT NULL DEFAULT NOW(),

    CHECK(char_length(title) BETWEEN 2 AND 200),
    CHECK(ends_at > starts_at),
    CHECK(status IN ('scheduled', 'ongoing', 'completed', 'cancelled'))
);
CREATE INDEX idx_classes_workspace_starts   ON classes(workspace_id, starts_at DESC);
CREATE INDEX idx_classes_tutor              ON classes(tutor_user_id, status, starts_at DESC);
CREATE INDEX idx_classes_upcoming           ON classes(starts_at) WHERE status = 'scheduled';
CREATE INDEX idx_classes_recurrence         ON classes(recurrence_parent_id) WHERE recurrence_parent_id IS NOT NULL;

CREATE TRIGGER set_updated_at_classes
BEFORE UPDATE ON classes
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column(); 

CREATE TABLE class_attendance(
    id                      UUID        PRIMARY KEY NOT NULL DEFAULT    gen_random_uuid(),
    class_id                UUID                    NOT NULL REFERENCES classes(id)  ON DELETE CASCADE,
    student_user_id         UUID                    NOT NULL REFERENCES users(id)    ON DELETE RESTRICT,
    status                  TEXT                    NOT NULL,
    marked_by_user_id       UUID                             REFERENCES users(id)    ON DELETE SET NULL,
    marked_at               TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    check_in_time           TIMESTAMPTZ,
    notes                   TEXT,   
    created_at              TIMESTAMPTZ             NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ             NOT NULL DEFAULT NOW(),


    CHECK(status IN ('present', 'absent', 'late', 'excused')),
    UNIQUE(class_id, student_user_id)
);
CREATE INDEX idx_class_attendance_student       ON class_attendance(student_user_id, marked_at DESC);

CREATE TRIGGER set_updated_at_class_attendance
BEFORE UPDATE ON class_attendance
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column(); 