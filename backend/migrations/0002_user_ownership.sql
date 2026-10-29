-- Existing shared categories must be cleared before applying this migration.
-- There is deliberately no guessed owner or automatic data reassignment.
CREATE TABLE users (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    telegram_id BIGINT NOT NULL UNIQUE CHECK (telegram_id > 0)
);

ALTER TABLE categories
    ADD COLUMN user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE RESTRICT;

CREATE INDEX categories_user_id_id_idx ON categories(user_id, id);
