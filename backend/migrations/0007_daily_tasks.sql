-- IDs are assigned lazily when a step is first selected for daily tracking.
CREATE SEQUENCE subtask_identity;

-- PostgreSQL resolves missing/ambiguous DST wall times using standard time.
CREATE FUNCTION daily_reset_at(zone TEXT, current_instant TIMESTAMPTZ) RETURNS TIMESTAMPTZ
LANGUAGE SQL STABLE STRICT AS $$
    SELECT CASE WHEN current_instant < (boundary AT TIME ZONE zone)
        THEN boundary AT TIME ZONE zone
        ELSE (boundary + interval '1 day') AT TIME ZONE zone END
    FROM (SELECT date_trunc('day', current_instant AT TIME ZONE zone)
        + interval '2 hours' AS boundary) b;
$$;
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
                OR step - ARRAY['id', 'description', 'completed']::text[] <> '{}'::jsonb
            ELSE TRUE END
        )
    ELSE FALSE END;
$$;

CREATE TABLE daily_tasks (
    task_id BIGINT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    subtask_id TEXT NOT NULL DEFAULT '',
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (task_id, subtask_id)
);
CREATE INDEX daily_tasks_expiry ON daily_tasks(expires_at);
