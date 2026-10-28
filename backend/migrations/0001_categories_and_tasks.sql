CREATE TABLE categories (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    description TEXT NOT NULL CHECK (description ~ '[^[:space:]]')
);

CREATE TABLE tasks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    category_id BIGINT NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    description TEXT NOT NULL CHECK (description ~ '[^[:space:]]')
);

CREATE INDEX tasks_category_id_idx ON tasks(category_id);
