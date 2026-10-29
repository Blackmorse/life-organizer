-- Legacy picks have no recorded timezone; retain them using UTC as fallback.
ALTER TABLE daily_tasks ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';

CREATE TABLE morning_offers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    timezone TEXT NOT NULL,
    reset_at TIMESTAMPTZ NOT NULL,
    due_at TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'sent', 'accepted', 'declined', 'expired')),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_id, timezone, reset_at)
);
CREATE INDEX morning_offers_due ON morning_offers(next_attempt_at, due_at) WHERE state = 'pending';
CREATE TABLE morning_offer_items (
    offer_id BIGINT NOT NULL REFERENCES morning_offers(id) ON DELETE CASCADE,
    task_id BIGINT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    subtask_id TEXT NOT NULL,
    PRIMARY KEY(offer_id, task_id, subtask_id)
);
CREATE TABLE bot_poll_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    next_update_id BIGINT NOT NULL DEFAULT 0
);
INSERT INTO bot_poll_state DEFAULT VALUES;

CREATE FUNCTION morning_offer_schedule(zone TEXT, reset_instant TIMESTAMPTZ)
RETURNS TABLE(due_at TIMESTAMPTZ, valid_until TIMESTAMPTZ)
LANGUAGE SQL STABLE STRICT AS $$
    SELECT (local_day + interval '8 hours') AT TIME ZONE zone,
        (local_day + interval '1 day 2 hours') AT TIME ZONE zone
    FROM (SELECT date_trunc('day', reset_instant AT TIME ZONE zone) AS local_day) d;
$$;

-- Every expiry path must capture picks, including lazy API cleanup and an
-- upsert replacing yesterday's pick before the background sweep gets to it.
CREATE FUNCTION preserve_expired_daily_pick() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    owner_id BIGINT;
    offer_id BIGINT;
    send_at TIMESTAMPTZ;
    ends_at TIMESTAMPTZ;
BEGIN
    IF OLD.expires_at <= statement_timestamp()
       AND (TG_OP = 'DELETE' OR NEW.expires_at IS DISTINCT FROM OLD.expires_at) THEN
        SELECT c.user_id INTO owner_id FROM tasks t JOIN categories c ON c.id = t.category_id
        WHERE t.id = OLD.task_id AND NOT t.completed
          AND (OLD.subtask_id = '' OR EXISTS (
              SELECT 1 FROM jsonb_array_elements(t.subtasks) s
              WHERE s->>'id' = OLD.subtask_id AND NOT (s->>'completed')::boolean));
        SELECT due_at, valid_until INTO send_at, ends_at FROM morning_offer_schedule(OLD.timezone, OLD.expires_at);
        IF owner_id IS NOT NULL AND ends_at > statement_timestamp() THEN
            INSERT INTO morning_offers(user_id, timezone, reset_at, due_at, valid_until)
            VALUES(owner_id, OLD.timezone, OLD.expires_at, send_at, ends_at)
            ON CONFLICT(user_id, timezone, reset_at) DO NOTHING
            RETURNING id INTO offer_id;
            IF offer_id IS NULL THEN
                SELECT id INTO offer_id FROM morning_offers
                WHERE user_id = owner_id AND timezone = OLD.timezone AND reset_at = OLD.expires_at;
            END IF;
            INSERT INTO morning_offer_items VALUES(offer_id, OLD.task_id, OLD.subtask_id) ON CONFLICT DO NOTHING;
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER daily_pick_rollover BEFORE DELETE OR UPDATE ON daily_tasks
FOR EACH ROW EXECUTE FUNCTION preserve_expired_daily_pick();
