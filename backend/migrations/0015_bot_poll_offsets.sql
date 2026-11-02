-- Telegram update IDs belong to a bot, not to a database. The legacy singleton
-- cursor cannot safely be assigned to a bot when its token may have changed.
-- Keep the old table for compatibility; new pollers use per-bot cursors.
CREATE TABLE bot_poll_offsets (
    bot_id BIGINT PRIMARY KEY,
    next_update_id BIGINT NOT NULL DEFAULT 0
);
