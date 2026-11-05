ALTER TABLE tasks ADD COLUMN detailed_description TEXT NOT NULL DEFAULT '';
ALTER TABLE scheduled_tasks ADD COLUMN detailed_description TEXT NOT NULL DEFAULT '';
ALTER TABLE scheduled_occurrences ADD COLUMN detailed_description TEXT NOT NULL DEFAULT '';
ALTER TABLE reminder_tasks ADD COLUMN detailed_description TEXT NOT NULL DEFAULT '';

-- Older steps may omit the optional field; supplied values must be strings.
CREATE OR REPLACE FUNCTION valid_task_subtasks(steps JSONB) RETURNS BOOLEAN
LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT CASE WHEN jsonb_typeof(steps) = 'array' THEN
        NOT EXISTS (
            SELECT 1 FROM jsonb_array_elements(steps) AS step
            WHERE CASE WHEN jsonb_typeof(step) = 'object' THEN
                jsonb_typeof(step->'description') IS DISTINCT FROM 'string'
                OR step->>'description' !~ '[^[:space:]]'
                OR jsonb_typeof(step->'completed') IS DISTINCT FROM 'boolean'
                OR (step ? 'id' AND (jsonb_typeof(step->'id') IS DISTINCT FROM 'string' OR step->>'id' !~ '^[0-9]+$'))
                OR (step ? 'detailed_description' AND jsonb_typeof(step->'detailed_description') IS DISTINCT FROM 'string')
                OR step - ARRAY['id', 'description', 'completed', 'detailed_description']::text[] <> '{}'::jsonb
            ELSE TRUE END
        )
    ELSE FALSE END;
$$;

CREATE OR REPLACE FUNCTION generate_scheduled_occurrences(owner_id BIGINT, instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE SQL AS $$
    INSERT INTO scheduled_occurrences
        (scheduled_task_id, planning_date, description, detailed_description, subtasks, expires_at, timezone)
    SELECT s.id, day.date, s.description, s.detailed_description,
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
