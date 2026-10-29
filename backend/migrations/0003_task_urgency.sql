-- Text values keep urgency levels easy to extend with a later migration.
-- Existing tasks have no implied deadline, so they start as someday.
ALTER TABLE tasks
    ADD COLUMN urgency TEXT NOT NULL DEFAULT 'someday',
    ADD CONSTRAINT tasks_urgency_check CHECK (urgency IN ('someday', 'upcoming', 'soon'));
