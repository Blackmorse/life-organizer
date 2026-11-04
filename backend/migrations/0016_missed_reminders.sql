-- A reminder can only belong to its event day's daily list, even if an old
-- selection or offer carries a later expiry.
UPDATE reminder_tasks SET selected_until = LEAST(selected_until,
    (((due_at AT TIME ZONE timezone)::date + 1) + time '02:00') AT TIME ZONE timezone)
WHERE selected_until IS NOT NULL;

CREATE FUNCTION limit_reminder_selection() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.selected_until IS NOT NULL THEN
        NEW.selected_until := LEAST(NEW.selected_until,
            (((NEW.due_at AT TIME ZONE NEW.timezone)::date + 1) + time '02:00')
                AT TIME ZONE NEW.timezone);
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER reminder_selection_expiry
BEFORE INSERT OR UPDATE OF selected_until, due_at, timezone ON reminder_tasks
FOR EACH ROW EXECUTE FUNCTION limit_reminder_selection();

ALTER TABLE morning_offer_reminder_items
    DROP CONSTRAINT morning_offer_reminder_items_kind_check;
ALTER TABLE morning_offer_reminder_items
    ADD CHECK (kind IN ('today', 'tomorrow', 'yesterday'));

CREATE OR REPLACE FUNCTION prepare_reminder_morning_offers(instant TIMESTAMPTZ)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO morning_offers(user_id, timezone, reset_at, due_at, valid_until)
    SELECT DISTINCT r.user_id, r.timezone,
        (d.day + time '02:00') AT TIME ZONE r.timezone,
        (d.day + time '08:00') AT TIME ZONE r.timezone,
        (d.day + 1 + time '02:00') AT TIME ZONE r.timezone
    FROM reminder_tasks r CROSS JOIN (VALUES (-1), (0), (1)) AS offsets(days)
    CROSS JOIN LATERAL (SELECT (r.due_at AT TIME ZONE r.timezone)::date - offsets.days AS day) d
    WHERE NOT r.completed AND (offsets.days = -1 OR r.selected_until IS NULL)
        AND (d.day + 1 + time '02:00') AT TIME ZONE r.timezone > instant
        AND (d.day + time '02:00') AT TIME ZONE r.timezone <= instant
    ON CONFLICT(user_id, timezone, reset_at) DO NOTHING;

    INSERT INTO morning_offer_reminder_items(offer_id, reminder_id, due_at, timezone, kind)
    SELECT m.id, r.id, r.due_at, r.timezone,
        CASE offsets.days WHEN -1 THEN 'yesterday' WHEN 0 THEN 'today' ELSE 'tomorrow' END
    FROM reminder_tasks r CROSS JOIN (VALUES (-1), (0), (1)) AS offsets(days)
    CROSS JOIN LATERAL (SELECT (r.due_at AT TIME ZONE r.timezone)::date - offsets.days AS day) d
    JOIN morning_offers m ON m.user_id = r.user_id AND m.timezone = r.timezone
        AND m.reset_at = (d.day + time '02:00') AT TIME ZONE r.timezone
    WHERE m.state = 'pending' AND m.valid_until > instant
        AND NOT r.completed AND (offsets.days = -1 OR r.selected_until IS NULL)
    ON CONFLICT DO NOTHING;
END;
$$;
