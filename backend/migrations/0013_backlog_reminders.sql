-- Follow-up delivery is independent of the offer decision, so failed sends can
-- retry without replaying acceptance or blocking Telegram update processing.
ALTER TABLE morning_offers
    ADD COLUMN backlog_ready BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN backlog_sent BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN backlog_next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE INDEX morning_backlog_due ON morning_offers(backlog_next_attempt_at)
    WHERE backlog_ready AND NOT backlog_sent;

-- Ordinary work still gets a morning reminder when there is nothing to carry
-- over. Reuse saved timezone history; UTC is the fallback for users who have
-- never supplied a timezone. Calendar arithmetic preserves local 08:00 on DST.
CREATE FUNCTION prepare_backlog_morning_offers(instant TIMESTAMPTZ)
RETURNS void LANGUAGE SQL AS $$
    INSERT INTO morning_offers(user_id, timezone, reset_at, due_at, valid_until)
    SELECT u.id, z.zone, d.reset_at, s.due_at, s.valid_until
    FROM users u
    CROSS JOIN LATERAL (SELECT COALESCE(
        (SELECT p.timezone FROM daily_tasks p JOIN tasks t ON t.id=p.task_id
         JOIN categories c ON c.id=t.category_id WHERE c.user_id=u.id
         ORDER BY p.expires_at DESC, t.id DESC LIMIT 1),
        (SELECT o.timezone FROM morning_offers o WHERE o.user_id=u.id
         ORDER BY o.reset_at DESC, o.id DESC LIMIT 1),
        (SELECT timezone FROM scheduled_tasks WHERE user_id=u.id ORDER BY id DESC LIMIT 1),
        (SELECT timezone FROM reminder_tasks WHERE user_id=u.id ORDER BY id DESC LIMIT 1),
        'UTC') AS zone) z
    CROSS JOIN LATERAL (SELECT
        (((daily_reset_at(z.zone, instant) AT TIME ZONE z.zone)::date - 1)
            + time '02:00') AT TIME ZONE z.zone AS reset_at) d
    CROSS JOIN LATERAL morning_offer_schedule(z.zone, d.reset_at) s
    WHERE s.due_at <= instant AND s.valid_until > instant
        AND NOT EXISTS (SELECT 1 FROM morning_offers o WHERE o.user_id=u.id
                        AND o.reset_at <= instant AND o.valid_until > instant)
        AND EXISTS (SELECT 1 FROM tasks t JOIN categories c ON c.id=t.category_id
                    WHERE c.user_id=u.id AND NOT t.completed)
    ON CONFLICT(user_id, timezone, reset_at) DO NOTHING;
$$;
