-- Tomorrow picks are independent of today's picks. At the target day's 02:00
-- they enter the same durable 08:00 offer flow as unfinished daily work.
CREATE TABLE tomorrow_tasks (
    task_id BIGINT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    subtask_id TEXT NOT NULL DEFAULT '',
    expires_at TIMESTAMPTZ NOT NULL,
    timezone TEXT NOT NULL,
    PRIMARY KEY(task_id, subtask_id)
);
CREATE INDEX tomorrow_tasks_expiry ON tomorrow_tasks(expires_at);

CREATE FUNCTION tomorrow_reset_at(zone TEXT, instant TIMESTAMPTZ)
RETURNS TIMESTAMPTZ LANGUAGE SQL STABLE STRICT AS $$
    SELECT (date_trunc('day', instant AT TIME ZONE zone)
        + interval '1 day 2 hours') AT TIME ZONE zone;
$$;

CREATE TRIGGER tomorrow_pick_rollover BEFORE DELETE OR UPDATE ON tomorrow_tasks
FOR EACH ROW EXECUTE FUNCTION preserve_expired_daily_pick();
