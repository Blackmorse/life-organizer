CREATE TABLE reminder_tasks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    description TEXT NOT NULL CHECK (btrim(description) <> ''),
    due_at TIMESTAMPTZ NOT NULL,
    timezone TEXT NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    selected_until TIMESTAMPTZ
);
CREATE INDEX reminder_tasks_owner ON reminder_tasks(user_id, due_at);

-- Snapshot the date/time and timezone so stale buttons cannot select rescheduled work.
CREATE TABLE morning_offer_reminder_items (
    offer_id BIGINT NOT NULL REFERENCES morning_offers(id) ON DELETE CASCADE,
    reminder_id BIGINT NOT NULL REFERENCES reminder_tasks(id) ON DELETE CASCADE,
    due_at TIMESTAMPTZ NOT NULL,
    timezone TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('today', 'tomorrow')),
    PRIMARY KEY (reminder_id, due_at, timezone, kind)
);
CREATE INDEX morning_offer_reminder_items_offer ON morning_offer_reminder_items(offer_id);

CREATE FUNCTION prepare_reminder_morning_offers(instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO morning_offers(user_id, timezone, reset_at, due_at, valid_until)
    SELECT DISTINCT r.user_id, r.timezone,
        (d.day + time '02:00') AT TIME ZONE r.timezone,
        (d.day + time '08:00') AT TIME ZONE r.timezone,
        (d.day + 1 + time '02:00') AT TIME ZONE r.timezone
    FROM reminder_tasks r CROSS JOIN (VALUES (0), (1)) AS offsets(days)
    CROSS JOIN LATERAL (SELECT (r.due_at AT TIME ZONE r.timezone)::date - offsets.days AS day) d
    WHERE NOT r.completed AND r.selected_until IS NULL
        AND (d.day + 1 + time '02:00') AT TIME ZONE r.timezone > instant
        AND (d.day + time '02:00') AT TIME ZONE r.timezone <= instant
    ON CONFLICT(user_id, timezone, reset_at) DO NOTHING;

    INSERT INTO morning_offer_reminder_items(offer_id, reminder_id, due_at, timezone, kind)
    SELECT m.id, r.id, r.due_at, r.timezone,
        CASE WHEN offsets.days = 0 THEN 'today' ELSE 'tomorrow' END
    FROM reminder_tasks r CROSS JOIN (VALUES (0), (1)) AS offsets(days)
    CROSS JOIN LATERAL (SELECT (r.due_at AT TIME ZONE r.timezone)::date - offsets.days AS day) d
    JOIN morning_offers m ON m.user_id = r.user_id AND m.timezone = r.timezone
        AND m.reset_at = (d.day + time '02:00') AT TIME ZONE r.timezone
    WHERE m.state = 'pending' AND m.valid_until > instant
        AND NOT r.completed AND r.selected_until IS NULL
    ON CONFLICT DO NOTHING;
END;
$$;
