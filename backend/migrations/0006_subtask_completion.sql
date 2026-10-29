-- Preserve existing step text and ordering; old steps start unfinished.
CREATE FUNCTION task_subtasks_with_completion(steps TEXT[]) RETURNS JSONB
LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT COALESCE(jsonb_agg(
        jsonb_build_object('description', description, 'completed', FALSE)
        ORDER BY position
    ), '[]'::jsonb)
    FROM unnest(steps) WITH ORDINALITY AS step(description, position);
$$;

ALTER TABLE tasks DROP CONSTRAINT tasks_subtasks_check;
ALTER TABLE tasks ALTER COLUMN subtasks DROP DEFAULT;
ALTER TABLE tasks ALTER COLUMN subtasks TYPE JSONB
    USING task_subtasks_with_completion(subtasks);
ALTER TABLE tasks ALTER COLUMN subtasks SET DEFAULT '[]'::jsonb;
DROP FUNCTION task_subtasks_with_completion(TEXT[]);
DROP FUNCTION valid_task_subtasks(TEXT[]);

CREATE FUNCTION valid_task_subtasks(steps JSONB) RETURNS BOOLEAN
LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT CASE WHEN jsonb_typeof(steps) = 'array' THEN
        NOT EXISTS (
            SELECT 1 FROM jsonb_array_elements(steps) AS step
            WHERE CASE WHEN jsonb_typeof(step) = 'object' THEN
                jsonb_typeof(step->'description') IS DISTINCT FROM 'string'
                OR step->>'description' !~ '[^[:space:]]'
                OR jsonb_typeof(step->'completed') IS DISTINCT FROM 'boolean'
                OR step - ARRAY['description', 'completed']::text[] <> '{}'::jsonb
            ELSE TRUE END
        )
    ELSE FALSE END;
$$;

ALTER TABLE tasks ADD CONSTRAINT tasks_subtasks_check
    CHECK (valid_task_subtasks(subtasks));
