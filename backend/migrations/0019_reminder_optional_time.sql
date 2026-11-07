-- Existing reminders retain their exact time. All-day reminders use due_at only
-- as a carrier for their calendar date in the saved timezone.
ALTER TABLE reminder_tasks ADD COLUMN all_day BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE morning_offer_reminder_items ADD COLUMN all_day BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE morning_offer_reminder_items DROP CONSTRAINT morning_offer_reminder_items_pkey;
ALTER TABLE morning_offer_reminder_items ADD PRIMARY KEY (reminder_id, due_at, timezone, kind, all_day);
