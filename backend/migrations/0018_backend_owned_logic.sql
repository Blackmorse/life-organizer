-- Business rules now execute explicitly in backend transactions. Keep historical
-- migrations intact so existing installations retain their SQLx checksums.
DROP TRIGGER daily_pick_rollover ON daily_tasks;
DROP TRIGGER tomorrow_pick_rollover ON tomorrow_tasks;
DROP TRIGGER reminder_selection_expiry ON reminder_tasks;

ALTER TABLE tasks DROP CONSTRAINT tasks_subtasks_check;
ALTER TABLE scheduled_tasks DROP CONSTRAINT scheduled_tasks_subtasks_check;
ALTER TABLE scheduled_occurrences DROP CONSTRAINT scheduled_occurrences_subtasks_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_subtasks_check CHECK (jsonb_typeof(subtasks) = 'array');
ALTER TABLE scheduled_tasks ADD CONSTRAINT scheduled_tasks_subtasks_check CHECK (jsonb_typeof(subtasks) = 'array');
ALTER TABLE scheduled_occurrences ADD CONSTRAINT scheduled_occurrences_subtasks_check CHECK (jsonb_typeof(subtasks) = 'array');

DROP FUNCTION preserve_expired_daily_pick();
DROP FUNCTION limit_reminder_selection();
DROP FUNCTION prepare_backlog_morning_offers(TIMESTAMPTZ);
DROP FUNCTION prepare_reminder_morning_offers(TIMESTAMPTZ);
DROP FUNCTION prepare_scheduled_morning_offers(TIMESTAMPTZ);
DROP FUNCTION generate_scheduled_occurrences(BIGINT, TIMESTAMPTZ);
DROP FUNCTION morning_offer_schedule(TEXT, TIMESTAMPTZ);
DROP FUNCTION tomorrow_reset_at(TEXT, TIMESTAMPTZ);
DROP FUNCTION daily_reset_at(TEXT, TIMESTAMPTZ);
DROP FUNCTION valid_task_subtasks(JSONB);
