-- Preserve existing templates, occurrences, and progress when upgrading 0009.
CREATE OR REPLACE FUNCTION generate_scheduled_occurrences(owner_id BIGINT, instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE SQL AS $$
    INSERT INTO scheduled_occurrences
        (scheduled_task_id, planning_date, description, subtasks, expires_at)
    SELECT s.id, day.date, s.description,
        COALESCE((SELECT jsonb_agg(step - 'id' || '{"completed":false}'::jsonb ORDER BY position)
            FROM jsonb_array_elements(s.subtasks) WITH ORDINALITY AS steps(step, position)), '[]'::jsonb),
        daily_reset_at(s.timezone, instant)
    FROM scheduled_tasks s
    CROSS JOIN LATERAL (SELECT
        (daily_reset_at(s.timezone, instant) AT TIME ZONE s.timezone)::date - 1 AS date) day
    WHERE (owner_id IS NULL OR s.user_id = owner_id)
        AND s.created_at <= instant
        AND extract(isodow FROM day.date)::smallint = ANY(s.weekdays)
    ON CONFLICT (scheduled_task_id, planning_date) DO NOTHING;
$$;

ALTER TABLE scheduled_tasks DROP COLUMN urgency;
ALTER TABLE scheduled_occurrences DROP COLUMN urgency;
