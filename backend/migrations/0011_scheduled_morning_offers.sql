-- Keep existing daily progress visible; future occurrences require opt-in.
ALTER TABLE scheduled_occurrences ADD COLUMN selected BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE scheduled_occurrences ALTER COLUMN selected SET DEFAULT FALSE;
ALTER TABLE scheduled_occurrences ADD COLUMN timezone TEXT;
UPDATE scheduled_occurrences o SET timezone = s.timezone
FROM scheduled_tasks s WHERE s.id = o.scheduled_task_id;
ALTER TABLE scheduled_occurrences ALTER COLUMN timezone SET NOT NULL;

CREATE TABLE morning_offer_scheduled_items (
    offer_id BIGINT NOT NULL REFERENCES morning_offers(id) ON DELETE CASCADE,
    occurrence_id BIGINT PRIMARY KEY REFERENCES scheduled_occurrences(id) ON DELETE CASCADE
);
CREATE INDEX morning_offer_scheduled_items_offer ON morning_offer_scheduled_items(offer_id);

CREATE OR REPLACE FUNCTION generate_scheduled_occurrences(owner_id BIGINT, instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE SQL AS $$
    INSERT INTO scheduled_occurrences
        (scheduled_task_id, planning_date, description, subtasks, expires_at, timezone)
    SELECT s.id, day.date, s.description,
        COALESCE((SELECT jsonb_agg(step - 'id' || '{"completed":false}'::jsonb ORDER BY position)
            FROM jsonb_array_elements(s.subtasks) WITH ORDINALITY AS steps(step, position)), '[]'::jsonb),
        daily_reset_at(s.timezone, instant), s.timezone
    FROM scheduled_tasks s
    CROSS JOIN LATERAL (SELECT
        (daily_reset_at(s.timezone, instant) AT TIME ZONE s.timezone)::date - 1 AS date) day
    WHERE (owner_id IS NULL OR s.user_id = owner_id)
        AND s.created_at <= instant
        AND extract(isodow FROM day.date)::smallint = ANY(s.weekdays)
    ON CONFLICT (scheduled_task_id, planning_date) DO NOTHING;
$$;

-- Share the same owner/timezone/planning-day offer as ordinary carryover.
-- Persist links before delivery so Yes accepts only the offered occurrences.
CREATE FUNCTION prepare_scheduled_morning_offers(instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO morning_offers(user_id, timezone, reset_at, due_at, valid_until)
    SELECT DISTINCT s.user_id, o.timezone,
        (o.planning_date + time '02:00') AT TIME ZONE o.timezone,
        (o.planning_date + time '08:00') AT TIME ZONE o.timezone, o.expires_at
    FROM scheduled_occurrences o JOIN scheduled_tasks s ON s.id = o.scheduled_task_id
    WHERE NOT o.selected AND NOT o.hidden AND NOT o.completed AND o.expires_at > instant
    ON CONFLICT(user_id, timezone, reset_at) DO NOTHING;

    INSERT INTO morning_offer_scheduled_items(offer_id, occurrence_id)
    SELECT m.id, o.id
    FROM scheduled_occurrences o JOIN scheduled_tasks s ON s.id = o.scheduled_task_id
    JOIN morning_offers m ON m.user_id = s.user_id AND m.timezone = o.timezone
        AND m.reset_at = (o.planning_date + time '02:00') AT TIME ZONE o.timezone
    WHERE m.state = 'pending' AND NOT o.selected AND NOT o.hidden AND NOT o.completed
        AND o.expires_at > instant
    ON CONFLICT(occurrence_id) DO NOTHING;
END;
$$;
