-- Snapshot identities keep rescheduled reminders from sending stale alerts.
CREATE TABLE reminder_alerts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reminder_id BIGINT NOT NULL REFERENCES reminder_tasks(id) ON DELETE CASCADE,
    due_at TIMESTAMPTZ NOT NULL,
    timezone TEXT NOT NULL,
    all_day BOOLEAN NOT NULL,
    slot SMALLINT NOT NULL CHECK (slot BETWEEN 0 AND 3),
    notify_at TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    retry_at TIMESTAMPTZ,
    sent BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (reminder_id, due_at, timezone, all_day, slot),
    CHECK (notify_at < valid_until)
);
CREATE INDEX reminder_alerts_pending ON reminder_alerts(notify_at, id) WHERE NOT sent;
