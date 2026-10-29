-- Subtasks are ordered text steps, part of the parent task's lifecycle.
CREATE FUNCTION valid_task_subtasks(steps TEXT[]) RETURNS BOOLEAN
LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT COALESCE(array_ndims(steps), 1) = 1
       AND NOT EXISTS (
           SELECT 1 FROM unnest(steps) AS step
           WHERE step IS NULL OR step !~ '[^[:space:]]'
       );
$$;

ALTER TABLE tasks ADD COLUMN subtasks TEXT[] NOT NULL DEFAULT '{}'
    CHECK (valid_task_subtasks(subtasks));
