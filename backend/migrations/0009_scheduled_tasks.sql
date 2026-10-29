CREATE TABLE scheduled_tasks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    description TEXT NOT NULL CHECK (description ~ '[^[:space:]]'),
    urgency TEXT NOT NULL DEFAULT 'someday' CHECK (urgency IN ('someday', 'upcoming', 'soon')),
    weekdays SMALLINT[] NOT NULL CHECK (
        cardinality(weekdays) BETWEEN 1 AND 7 AND array_ndims(weekdays) = 1
        AND weekdays <@ ARRAY[1,2,3,4,5,6,7]::smallint[]
        AND array_position(weekdays, NULL) IS NULL),
    timezone TEXT NOT NULL,
    subtasks JSONB NOT NULL DEFAULT '[]' CHECK (valid_task_subtasks(subtasks)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT statement_timestamp()
);
CREATE INDEX scheduled_tasks_owner ON scheduled_tasks(user_id, id);

-- One snapshot per planning day. Hidden rows retain identity so removal cannot
-- regenerate an occurrence on refresh. Completion never changes the template.
CREATE TABLE scheduled_occurrences (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    scheduled_task_id BIGINT NOT NULL REFERENCES scheduled_tasks(id) ON DELETE CASCADE,
    planning_date DATE NOT NULL,
    description TEXT NOT NULL,
    urgency TEXT NOT NULL CHECK (urgency IN ('someday', 'upcoming', 'soon')),
    subtasks JSONB NOT NULL CHECK (valid_task_subtasks(subtasks)),
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    hidden BOOLEAN NOT NULL DEFAULT FALSE,
    expires_at TIMESTAMPTZ NOT NULL,
    UNIQUE (scheduled_task_id, planning_date)
);
CREATE INDEX scheduled_occurrences_expiry ON scheduled_occurrences(expires_at);

CREATE FUNCTION generate_scheduled_occurrences(owner_id BIGINT, instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE SQL AS $$
    INSERT INTO scheduled_occurrences
        (scheduled_task_id, planning_date, description, urgency, subtasks, expires_at)
    SELECT s.id, day.date, s.description, s.urgency,
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
