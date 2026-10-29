# Backend Guide

## Purpose

This Rust subproject provides the backend server for the personal
task-organizing app. It owns task-management business logic, exposes an API to
the React Telegram Mini App and possible standalone web UI, and initially hosts
the Telegram bot for notifications and possible message-based interactions.

The backend will manage tasks, categories, subcategories, and implementation
plans. PostgreSQL is the chosen relational database for persistent storage.

## Current state

- Rust edition 2024; Axum 0.8 HTTP server running on Tokio.
- SQLx 0.8 provides PostgreSQL pooling, queries, and embedded versioned migrations.
- Category and task CRUD are implemented through DAO, service, and API layers.
- Database migrations run automatically before the server accepts requests.
- Telegram Mini App authentication verifies signed `initData` on every API request.
  Users are registered automatically; category/task operations are user-scoped.
- Telegram bot messaging, subcategories, and implementation plans are not
  implemented yet. The bot messaging library remains undecided.

### Module layout

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Bootstrap configuration, shared connection pool, services, listener, and shutdown. |
| `src/config.rs` | Read environment-based server configuration. |
| `src/auth.rs` | Telegram initData HMAC verification, freshness checks, and signed identity extraction. |
| `src/api/auth.rs` | Authentication middleware, user registration, and per-user service wiring. |
| `src/db.rs` | Create the PostgreSQL pool and apply migrations; reused by tests. |
| `src/domain.rs` | Category and task data types, independent of SQLx and Axum. |
| `src/dao/` | DAO traits and PostgreSQL implementations, one entity per module. |
| `src/service/` | Transport-independent CRUD, input validation, and application errors. |
| `src/api/` | Axum routing, JSON inputs, response status codes, and error mapping. |
| `migrations/` | Append-only SQLx schema migrations embedded in the binary. |
| `tests/` | DAO, service, and in-process router integration tests. |
| `tests/support/` | Fresh Testcontainers PostgreSQL setup and test wiring. |

### Database schema

- `users`: generated `id BIGINT` primary key, positive `telegram_id BIGINT NOT NULL UNIQUE`.
- `categories`: `id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY`,
  `user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE RESTRICT`,
  `description TEXT NOT NULL`. `(user_id, id)` is indexed.
- `tasks`: `id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY`,
  `category_id BIGINT NOT NULL REFERENCES categories(id) ON DELETE RESTRICT`,
  `description TEXT NOT NULL`.
- Descriptions must not be blank; they are not unique keys.
- Each task belongs to exactly one category; categories can contain many tasks.
- Category owners use internal user IDs, not Telegram IDs. Tasks inherit their
  owner through their category; there is no duplicate task ownership column.
- Migration `0002_user_ownership.sql` assumes categories are empty. Clear old
  shared tasks/categories before upgrading; no legacy owner is guessed and no
  data is deleted by the migration.
- `tasks.category_id` is indexed. Tasks can be moved by updating `category_id`.
- A category containing tasks cannot be deleted. Delete or move its tasks first.
- Add new numbered SQL files for schema changes; do not edit migrations already
  applied to persistent databases. SQLx records applied versions and checksums.
- `build.rs` causes migration changes to trigger recompilation.

### HTTP API

All routes require `Authorization: tma <raw Telegram.WebApp.initData>`. The
backend validates Telegram's two-stage HMAC-SHA-256 using `TELEGRAM_BOT_TOKEN`,
compares the hash in constant time, rejects duplicate fields, and requires a
positive signed user ID and `auth_date` no older than 3600 seconds (at most 30
seconds ahead of server time). Missing/invalid/expired auth returns `401` JSON.
Close and reopen the Mini App to obtain fresh initData; there is no separate
session token or development bypass. Do not log authorization headers or initData.

Verified Telegram IDs are atomically upserted into `users`. Middleware builds
services with immutable user-scoped DAOs for each request. All reads and writes
filter by that owner; inaccessible records return `404`, and category filters
for another user's category return an empty list. Both source and destination
ownership are checked in the SQL for task moves. Request bodies cannot select
an owner. Authenticated responses use `Cache-Control: no-store`.

| Method | Path | Result |
| --- | --- | --- |
| GET | `/me` | Current user `{id, telegram_id}` (`200`). |
| POST | `/categories` | Create category (`201`). |
| GET | `/categories` | List categories ordered by ID (`200`). |
| GET | `/categories/{id}` | Read category (`200`). |
| PUT | `/categories/{id}` | Replace category description (`200`). |
| DELETE | `/categories/{id}` | Delete empty category (`204`). |
| POST | `/tasks` | Create task (`201`). |
| GET | `/tasks` | List tasks ordered by ID (`200`); optional `?category_id=123`. |
| GET | `/tasks/{id}` | Read task (`200`). |
| PUT | `/tasks/{id}` | Replace description and category (`200`). |
| DELETE | `/tasks/{id}` | Delete task (`204`). |

Category POST/PUT JSON: `{"description":"Work"}`.
Task POST/PUT JSON: `{"category_id":1,"description":"Build backend"}`.
Responses include generated `id` fields. IDs must be positive. A nonexistent
category filter returns an empty list. Category responses also include `user_id`.

Service errors use `{"error":"message"}`: invalid input `400`, missing entity or
referenced category `404`, nonempty category deletion `409`, storage failure
`500` with no database details exposed. Malformed JSON/path/query inputs use
Axum's extractor rejection responses (including `400`, `415`, and `422`).

## Architecture and modularity

- Split independent responsibilities into focused Rust modules as functionality grows.
- Keep `main.rs` focused on startup, configuration, and wiring modules together.
- Keep core domain and application logic independent of HTTP, Telegram, and PostgreSQL details.
- API handlers and bot interactions must reuse the same application logic.
- Keep persistence, API transport, and Telegram integration in separate modules.
- Prefer clear module boundaries within this crate; introduce additional crates only when justified.
- Share one connection pool. Enforce relationships in PostgreSQL rather than
  relying on existence checks that can race with concurrent writes.
- Services depend on DAO traits; keep handlers thin and reuse services for future bot interactions.

## Testing

- Cover implemented behavior with appropriate unit and integration tests.
- Keep domain logic testable independently of external services.
- Each database-backed test starts its own `postgres:16-alpine` Testcontainers
  container and applies production migrations using `db::connect_and_migrate`.
  Do not share containers or reset tables between tests. Docker must be running.
- Current tests exercise DAOs, services, and Axum's router in-process against
  real PostgreSQL. They do not launch the backend executable or a network server.
- Include end-to-end coverage of complete backend workflows as the application develops.
- End-to-end tests are primarily maintained by the human project owner.
- Agents must not create, modify, delete, disable, or regenerate end-to-end tests,
  fixtures, snapshots, expected results, or their harness/configuration without
  an explicit human request specifically authorizing that work. A general
  feature request or failing test does not provide that authorization.
- Agents may run existing end-to-end tests and report failures; do not weaken
  assertions to make implementation changes pass.

## Commands

Run from `backend/`:

```sh
DATABASE_URL=postgres://user:password@localhost:5432/scheduler TELEGRAM_BOT_TOKEN='<your BotFather token>' cargo run
cargo build
cargo check
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

`TELEGRAM_BOT_TOKEN` is required and must belong to the bot launching this Mini App.
Keep it backend-only; never use a `VITE_*` variable for it.
`DATABASE_URL` is required for application startup and must point to an existing
PostgreSQL database; the database user needs schema migration permissions.
`BIND_ADDRESS` defaults to `127.0.0.1:3000`. Tests create their own databases and
do not need `DATABASE_URL`. Run individual suites with `cargo test --test dao`,
`cargo test --test services`, or `cargo test --test api`.

No separate human-maintained end-to-end suite exists yet. Document its commands
when introduced with explicit human authorization.

Keep `Cargo.lock` version-controlled for reproducible application builds.
