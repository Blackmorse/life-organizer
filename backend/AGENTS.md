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
- Task urgency is implemented end-to-end: Someday (default), Upcoming, and Soon.
- Tasks support optional ordered subtasks with descriptions and completion flags,
  replaced atomically. A nonempty fully completed list auto-completes the parent.
- Task completion/restoration is implemented. Completed tasks form a read-only
  archive; restore before editing, moving, changing subtasks, or deleting.
- Database migrations run automatically before the server accepts requests.
- Category-free scheduled tasks use required weekday schedules and saved IANA
  timezones, with independently completable daily occurrences and optional subtasks.
- Telegram Mini App authentication verifies signed `initData` on every API request.
  Users are registered automatically; category/task operations are user-scoped.
- Daily-list finalization sends a summary to the verified user's private Telegram
  chat through the existing bot token, using reqwest with rustls. A background
  worker sends 08:00 rollover offers and polls Telegram messages and callback
  queries for private-chat commands and Yes/No responses. Subcategories and implementation plans
  are not implemented yet.

### Module layout

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Bootstrap configuration, shared connection pool, services, listener, and shutdown. |
| `src/config.rs` | Read environment-based server configuration. |
| `src/auth.rs` | Telegram initData HMAC verification, freshness checks, and signed identity extraction. |
| `src/telegram.rs` | Outbound Telegram Bot API adapter, bounded requests, sanitized delivery errors. |
| `src/telegram/morning.rs` | Morning-offer buttons, command-menu registration, message/callback polling and acknowledgements. |
| `src/morning.rs` | Morning-offer orchestration, periodic delivery, command dispatch, durable update processing, singleton worker lifecycle. |
| `src/bot_commands.rs` | Private-chat `/today`, `/start`, and `/help` commands using user-scoped application services. |
| `src/dao/morning.rs` | Morning-offer storage, claiming, ownership checks, atomic acceptance, and polling offset. |
| `src/dao/scheduled.rs`, `src/service/scheduled.rs`, `src/api/scheduled.rs` | Recurring templates, daily snapshots, validation, ownership, and scheduled-task routes. |
| `src/service/notification.rs` | Transport-independent message sender boundary and daily summary formatting. |
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
  `description TEXT NOT NULL`, `urgency TEXT NOT NULL DEFAULT 'someday'` with a
  check constraint allowing `someday`, `upcoming`, and `soon`.
- Task urgency describes a planning horizon, not a deadline or completion state:
  Someday = at some point in the future; Upcoming = within the next few weeks;
  Soon = within the next few days. Migration `0003_task_urgency.sql` assigns
  Someday to existing tasks without deleting data.
- Urgency is a transport-independent `TaskUrgency` enum persisted as text.
  Extend levels by adding a new migration for the check constraint, updating
  the domain enum/string conversion, and updating the frontend urgency catalog.
- Descriptions must not be blank; they are not unique keys.
- `tasks.subtasks`: `JSONB NOT NULL DEFAULT '[]'`, an ordered array of
  `{description: string, completed: boolean}` objects. Migration
  `0004_task_subtasks.sql` originally introduced `TEXT[]` steps;
  `0006_subtask_completion.sql` converts existing text to objects, preserving order
  and duplicates, with `completed: false`. Parent completion state is preserved.
  A database check enforces an array of objects with these required fields and
  optional string `id`, nonblank descriptions, and boolean flags.
  SQLx uses `Json<Vec<Subtask>>`.
  Subtasks may have a stable task-owned string `id`, assigned lazily on first
  daily selection. Clients must preserve returned IDs on edits and reordering;
  unknown or duplicate IDs are rejected. Unselected steps may omit `id`.
  Subtasks have no independent categories, urgency, archive, or nested children.
  They inherit the parent's ownership and lifecycle; deleting the task deletes steps.
- Each ordinary task belongs to exactly one category; categories can contain many tasks.
- `tasks.completed`: `BOOLEAN NOT NULL DEFAULT FALSE`, added by migration
  `0005_task_completion.sql`. Existing/new tasks start active. Completion preserves
  description, category, urgency, and subtasks; restoration only resets this flag.
  Archive is a task state, not a separate table or category.
- Category owners use internal user IDs, not Telegram IDs. Tasks inherit their
  owner through their category; there is no duplicate task ownership column.
- Migration `0002_user_ownership.sql` assumes categories are empty. Clear old
  shared tasks/categories before upgrading; no legacy owner is guessed and no
  data is deleted by the migration.
- `tasks.category_id` is indexed. Tasks can be moved by updating `category_id`.
- A category containing tasks (including archived tasks) cannot be deleted.
  Restore archived tasks before moving or deleting them.
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
| GET | `/scheduled-tasks` | List owned recurring templates (`200`). |
| POST | `/scheduled-tasks` | Create a category-free recurring template (`201`). |
| PUT | `/scheduled-tasks/{id}` | Replace template details/schedule (`200`). |
| DELETE | `/scheduled-tasks/{id}` | Delete template and its occurrences (`204`). |
| GET | `/scheduled-tasks/daily` | Generate candidates and list today's accepted, visible, unexpired occurrences (`200`). |
| PUT | `/scheduled-tasks/daily/{id}/completion` | Set occurrence completion; return current occurrences (`200`). |
| PUT | `/scheduled-tasks/daily/{id}/subtasks` | Replace occurrence steps and recalculate completion; return occurrences (`200`). |
| DELETE | `/scheduled-tasks/daily/{id}` | Skip occurrence for today only; return current occurrences (`200`). |
| GET | `/daily-tasks` | Current user's unexpired daily picks (`200`). |
| POST | `/daily-tasks` | Select all task subtasks (or the task if none), or one subtask; return current picks (`200`). |
| DELETE | `/daily-tasks` | Remove one step or all picks for a task; return current picks (`200`). |
| POST | `/daily-tasks/finalize` | Send the current daily list to the verified user's private bot chat (`204`). |
| POST | `/categories` | Create category (`201`). |
| GET | `/categories` | List categories ordered by ID (`200`). |
| GET | `/categories/{id}` | Read category (`200`). |
| PUT | `/categories/{id}` | Replace category description (`200`). |
| DELETE | `/categories/{id}` | Delete empty category (`204`). |
| POST | `/tasks` | Create task (`201`). |
| GET | `/tasks` | List all active and archived tasks ordered by ID (`200`); optional `?category_id=123`. |
| GET | `/tasks/{id}` | Read task (`200`). |
| PUT | `/tasks/{id}` | Replace description, category, and urgency (`200`). |
| DELETE | `/tasks/{id}` | Delete task (`204`). |
| PUT | `/tasks/{id}/subtasks` | Replace ordered subtasks and return updated task (`200`). |
| PUT | `/tasks/{id}/completion` | Complete/archive or restore task; return updated task (`200`). |

Category POST/PUT JSON: `{"description":"Work"}`.

Daily POST JSON: `{"task_id":1,"timezone":"Europe/Berlin"}`; optional
`"subtask_index":0` selects a zero-based step. Without an index, a task with
subtasks selects every current step instead of the parent; completed flags are
preserved. Tasks without steps select the parent. Bulk selection assigns any
missing stable IDs atomically, replaces a legacy whole-task pick, and upserts
steps without duplicates. The backend
validates the timezone against PostgreSQL's timezone catalog, locks the owned
active parent, assigns stable step IDs if needed, and upserts picks.
Missing/inaccessible tasks or steps return `404`; archived parents return `409`.
Daily DELETE JSON: `{"task_id":1,"subtask_id":""}` removes all picks for that task,
or use the returned string step ID to remove only that step. Removal is idempotent, scoped, and allowed after
completion. Inputs reject unknown fields. Responses are arrays of
`{task_id, subtask_id, expires_at}`, where `expires_at` is Unix milliseconds.
Migration `0007_daily_tasks.sql` adds the daily table and optional step identities.
Expiry is the next local 02:00 at selection time; calendar-day arithmetic handles
DST (PostgreSQL standard-time resolution for missing/ambiguous wall times).
Reads exclude expired picks even when no app was running and lazily clean them.
Completed picks remain visible until removed/reset. Deleted tasks cascade-delete
picks; removed steps drop their picks atomically during subtask replacement.
Task edits/moves and step edits/reordering retain picks. The API hides expired
picks immediately; a background sweep also physically removes them.

Finalization accepts no input fields: recipient is the authenticated user's
verified Telegram ID, never a client-supplied chat ID. One user-scoped SQL snapshot
resolves unexpired picks with current task/subtask descriptions and completion
flags, ordered by task ID and current subtask order. The application formats a
HTML "Tasks for today are ready!" list with bold 📁 category headings, indented
bold task headings, and selected subtasks indented one level deeper. Category-free
scheduled work appears under 🔁 Scheduled, including its occurrence subtasks.
Whole-task picks appear once; when both a legacy task pick and its steps
are picked, steps appear below the same heading. Selected work uses 🔹 for
unfinished picks and ✅ for completed picks. Context-only parents have no status
emoji; they use 📋 to identify the parent task instead. Groups use category and
task IDs so duplicate names stay separate.
Morning offers share the same category/task hierarchy. All descriptions are
HTML-escaped, including multiline category names. The adapter uses `parse_mode: "HTML"`
without inline keyboards. Long summaries split at 4096 rendered UTF-16 units,
preserving full descriptions with balanced tags and intact entities.
Empty/expired lists return `400` without sending. Unstarted/blocked bot chats return
`400` with Start/unblock guidance; other delivery errors return sanitized `502`.
The UI reports success only after all chunks are accepted by Telegram. On partial
delivery or ambiguous network failure, users are told to check the chat before
retrying. No application-level automatic retries or exactly-once guarantee;
explicit repeated finalization sends the current list again. Picks, expiry, and
task progress are unchanged by finalization. The shared HTTP client has 5-second
connect and 15-second per-request timeouts and does not follow redirects.
Bot users must first open the private chat and press Start/send `/start`, and must
not block the bot. No additional token or chat-ID environment variable is needed.

### Private-chat bot commands

- `/today` sends the current server-resolved Tasks of the day, including accepted
  scheduled occurrences, using the same category hierarchy, completion flags,
  escaping, and message splitting as Mini App finalization. Empty lists get a
  short reply explaining how to add work. It does not select or complete tasks.
- `/start` and `/help` explain `/today` and direct users to the Mini App for edits.
- The elected worker registers these commands with Telegram's `setMyCommands`
  using `all_private_chats` scope at startup. Registration failure is logged
  without stopping typed commands; restarting retries menu setup.
- Incoming Telegram sender identity determines the user and reply recipient.
  Only positive sender IDs matching a private chat ID are handled. Group chats,
  mismatched identities, bot senders, non-text/edited messages, and unrelated text
  are ignored. Bare commands are supported; addressed `@bot` forms are ignored.
- Commands reuse `DailyService::current_messages` and the owned daily DAO.
  Their update offset advances even on delivery failure to avoid blocking later
  callbacks or replaying partially sent summaries. Send `/today` again to retry.

### Scheduled tasks

Migration `0009_scheduled_tasks.sql` adds directly user-owned `scheduled_tasks`
templates and `scheduled_occurrences` snapshots, without categories or synthetic
category records. Template POST/PUT example:

```json
{"description":"Sport exercises","weekdays":[1,3,5],"timezone":"Europe/Berlin","subtasks":[{"description":"Warm up","completed":false}]}
```

Weekdays are distinct ISO day numbers (Monday=1 through Sunday=7), with at least
one required; all seven means every day. Timezone is required and validated
against PostgreSQL's timezone catalog. Subtasks default to an empty list.
Scheduled tasks have no urgency; migration `0010_scheduled_tasks_without_urgency.sql`
removes urgency from templates and occurrences while preserving existing progress.
Templates return the input fields plus generated `id`.
Template step flags are normalized to false; step IDs are not accepted.
Descriptions and step descriptions must be nonblank. Unknown fields are rejected.

`generate_scheduled_occurrences(owner_id, instant)` generates only the current
planning day, using `daily_reset_at` and local calendar dates (including DST).
Planning days start at 02:00; before then, the previous weekday still applies.
An occurrence snapshots description, timezone, and steps, all initially unchecked.
New occurrences are unselected candidates, absent from daily reads and summaries
until the owner accepts their morning offer. Progress writes and removal require selection.
Uniqueness on `(scheduled_task_id, planning_date)` prevents duplicates under
concurrent sweeps/requests. Generation runs every 30 seconds in the existing worker,
on scheduled daily reads, and before summary resolution. Restart catches up today's
due candidates only; missed days are not backfilled. New due templates generate
candidates on the next daily read, but never automatically join the daily list.

Daily responses contain `{id, scheduled_task_id, description, subtasks,
completed, expires_at}` with expiry in Unix milliseconds. Completion accepts
`{"completed":true}`; subtask writes accept the same object list as ordinary tasks,
but without stable IDs. A nonempty fully checked list completes only that
occurrence. Occurrences remain editable/reopenable until expiry and never archive
their template. Expired, hidden, missing, or inaccessible occurrences return `404`
for progress writes. Removal sets a hidden tombstone so refresh cannot regenerate
today's occurrence. Template edits affect future snapshots, preserving today's
progress; template deletion cascades to all its occurrences. Occurrence snapshots
remain stored after expiry but are excluded from all active reads and summaries.

Finalization includes selected visible unexpired occurrences with ordinary picks in the
same SQL snapshot. Internal negative occurrence IDs separate summary groups from
positive ordinary task IDs; API identities remain positive and use separate routes.
Migration `0011_scheduled_morning_offers.sql` adds selection state, snapshot timezones,
and durable `morning_offer_scheduled_items` links. Existing occurrences retain their
selection and progress; future occurrences require opt-in. Each due planning day
gets an 08:00 offer in its saved timezone, even without ordinary carryover. Matching
owner/timezone/reset offers combine both types of work. Yes selects only linked,
still-owned, unexpired, unfinished, unhidden occurrences atomically with ordinary
picks. No leaves candidates unselected. Scheduled occurrences never carry over;
future due days get fresh candidates regardless of previous decisions/progress.
Candidates created after an offer is sent or answered are not added to that offer.

### Morning carryover offers

Migration `0008_morning_offers.sql` stores the IANA timezone on each daily pick.
Legacy picks fall back to UTC; selecting them again records the current device
timezone. A database trigger captures unfinished picks when expired rows are
deleted or replaced, including lazy API cleanup and selection before the sweep.
Removed active picks do not create offers. `morning_offers` groups by owner,
timezone, and reset instant; `morning_offer_items` retains stable task/step links.
This preserves yesterday's work without keeping it in today's active list.

At 08:00 local time on the reset date, the bot sends the still-unfinished work
and any linked scheduled candidates due that day
with Yes/No inline buttons. Local calendar arithmetic and PostgreSQL timezone
rules apply, including DST. Completed/deleted work is filtered again before
delivery and acceptance. Empty offers send nothing. Yes atomically adds eligible
picks with their next local 02:00 expiry; already-active picks keep their expiry.
No consumes the offer without changing picks. Repeated choices are idempotent;
offers expire at the following local 02:00 and cannot be accepted afterward.
Task locks serialize acceptance with app edits/completion. Only the owning user's
callback from their private chat is accepted, using identity returned by Telegram.
Reopen or refresh the Mini App to load picks restored through the bot.

The backend starts a worker automatically with the existing bot token. It sweeps
every 30 seconds and polls `getUpdates` every 2 seconds for `callback_query` and
`message` updates.
A dedicated PostgreSQL advisory-lock session elects one worker per database;
other instances wait. Keep this token's webhook disabled and do not run another
poller for the same bot. No webhook endpoint or additional service is required.
`bot_poll_state` persists the offset after each processed update. Offer decisions
commit before callback acknowledgement, so replay cannot repeat an acceptance.
Pending deliveries use a five-minute retry lease. A network failure after Telegram
accepted a message may produce a duplicate prompt, but its buttons still share one
idempotent offer. All error logs omit bot URLs/tokens. Shutdown cancels the worker
and closes its advisory-lock session. The backend must run to send reminders;
restart catches up only offers still within their validity window.

Completion PUT JSON: `{"completed":true}` archives; `{"completed":false}` restores.
This endpoint requires a boolean and rejects unknown fields. Both operations are
idempotent and user-scoped. Task responses always include `completed`; ordinary
task POST creates an active task and PUT does not change completion state.
Task GET/list include archived data so clients can display a separate archive.
Archived tasks reject ordinary PUT (including category/urgency changes), subtask
replacement, and DELETE with `409` and
`{"error":"archived tasks are read-only; restore the task first"}`. Inaccessible
tasks still return `404`. DAO writes lock the user-owned task row in a transaction
before checking state, serializing edits/deletes against completion; a preceding
service-layer lookup alone would allow racing writes to mutate archived tasks.
Subtasks PUT JSON:
`{"subtasks":[{"description":"Design schema","completed":true},{"description":"Build UI","completed":false}]}`.
Send an empty list to remove all subtasks. The field is required on this route;
every step requires a string description and boolean completed flag. Unknown step
fields, legacy string entries, null, and malformed objects are rejected. Every
description must be nonblank. New UI steps start with `completed: false`.
Task responses always include `subtasks` (an empty array by default). Ordinary
task POST creates an empty list; ordinary task PUT preserves existing subtasks,
including when moving categories. List replacement is atomic and last-write-wins.
Saving a nonempty list where every step is complete also sets parent `completed`
to true in the same database write. This includes deleting the last unfinished
step from a list with finished steps remaining. Empty lists stay active. Finished
steps remain in place; they have no separate archive and can be unchecked while
the parent is active. Archived parents still reject all subtask writes with `409`.
Restore preserves subtask flags and leaves the task active; a subsequent subtask
write recalculates completion. Manual parent completion does not change step flags.
The same ownership checks apply; missing/inaccessible parent tasks return `404`.
Task POST/PUT JSON: `{"category_id":1,"description":"Build backend","urgency":"upcoming"}`.
Task responses always include `urgency`. Omitted urgency defaults to `someday`
on both POST and PUT (PUT replaces the task); null and unknown values are rejected
by the JSON extractor. Clients should send the selected urgency on every write.
Responses include generated `id` fields. IDs must be positive. A nonexistent
category filter returns an empty list. Category responses also include `user_id`.

Service errors use `{"error":"message"}`: invalid input `400`, missing entity or
referenced category `404`, nonempty category deletion or archived task write `409`, storage failure
`500` with no database details exposed. Malformed JSON/path/query inputs use
Axum's extractor rejection responses (including `400`, `415`, and `422`).

### Bot-to-Mini-App links (discussed, not implemented)

Future bot messages can use `sendMessage` with inline URL buttons or text links
to launch a Mini App at a category/task. One option, after configuring a Main
Mini App in BotFather, is `https://t.me/<bot_username>?startapp=category_42`.
The payload format is illustrative, not an implemented contract. A private-chat
`web_app` inline button can instead target the HTTPS app URL with a destination
query parameter. Main Mini App setup is separate from the existing menu button.

Keep message sending and link construction in Telegram-specific integration;
reuse user-scoped application services for selecting records. Destination IDs
are navigation hints, not access grants. Existing Mini App authentication and
ownership checks must apply when the frontend loads the destination.

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
- Telegram adapter tests use a local mock HTTP server; API tests inject a recording
  sender. Tests never send real Telegram messages or need a real bot token.
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
