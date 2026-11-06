# Project Guide

## Purpose

Build a personal task-organizing app. Users should be able to track tasks,
organize them into categories and subcategories, and maintain implementation
plans for tasks.

## Architecture

This repository is a monorepo with two subprojects in its intended layout:

| Directory | Stack | Responsibilities |
| --- | --- | --- |
| `backend/` (initialized) | Rust | Backend API, task-management business logic, PostgreSQL persistence, and Telegram bot. |
| `frontend/` (initialized) | React / TypeScript / Vite | Task/category UI, adaptable to Telegram Mini App and standalone browser use. |

There are three logical app components but initially only two subprojects:

1. **Backend API:** Provides task, category, subcategory, and implementation-plan functionality to the UI. Owns business rules and persistence.
2. **Telegram Mini App / web UI:** Main user-facing interface for viewing and managing tasks and their organization. Runs inside Telegram initially; standalone web support is a possible extension.
3. **Telegram bot:** Sends notifications and may support additional task interactions through Telegram messages or commands. Initially lives within the Rust backend rather than a separate subproject or service.

### Component boundaries

- The React frontend communicates with the backend through its API.
- The backend is the source of truth for task data and business rules.
- API handlers and bot interactions should reuse backend application logic.
- Keep Telegram-specific integration separate from core task-management logic.
- Keep frontend code adaptable to Telegram Mini App and potential standalone web contexts.

## Core concepts

- **Tasks:** Personal work items to track and manage, with urgency indicating
  their planning horizon: Someday, Upcoming (weeks), or Soon (days).
- **Categories and subcategories:** Hierarchical organization of tasks.
- **Implementation plans:** Plans describing how to carry out a task.
- **Detailed descriptions:** Optional plain-text notes on tasks and subtasks,
  empty by default. Visible only in expanded Mini App details; existing short
  descriptions remain the names used in lists and Telegram messages.
- **Subtasks:** Optional ordered steps with descriptions and completion flags,
  managed as a single list. Completed steps stay visible; completing every step
  in a nonempty list automatically completes the parent task.
- **Completion/archive:** Completing a task moves it out of the active view into
  a read-only archive. Restore is the only allowed change to an archived task.
- **Notifications:** Daily-list summaries and an 08:00 offer to carry unfinished picks into the new day through Yes/No bot buttons.
- **Scheduled tasks:** Category-free recurring templates with required weekdays
  and saved IANA timezone. Due planning days receive independent daily occurrences.
- **Reminder tasks:** Category-free one-time work with an exact date/time and saved
  IANA timezone, without urgency. Morning messages give a day-before heads-up and
  offer same-day inclusion in Tasks of the day.

## Current repository stage

`backend/` is a Rust Axum/Tokio server with PostgreSQL storage through SQLx.
Category and task CRUD are available through DAO, service, and HTTP API layers.
Users have surrogate IDs and unique Telegram IDs; each category belongs to one
user, and tasks inherit ownership from their category. The API verifies signed
Telegram Mini App `initData` and scopes every operation to that user.
Each ordinary task belongs to exactly one category and has urgency (Someday, Upcoming,
or Soon), defaulting to Someday. Each task can contain an ordered subtask list,
with completion flags and atomic list replacement through a user-scoped API.
Saving a nonempty fully completed subtask list automatically completes the parent.
Completion archives tasks;
archived task edits, moves, subtask writes, and deletion are rejected until restored.
Versioned SQLx migrations run at
startup. Database-backed integration tests use a fresh Testcontainers PostgreSQL
container per test. Backend-specific architecture, API contract, testing rules,
and build/run/test commands live in `backend/AGENTS.md`.

`frontend/` is a React/TypeScript/Vite app using Material UI, with a read-first,
searchable category/task hierarchy, CRUD, task reassignment, and urgency
selection/badges, and ordered subtask editing/reordering with completion checkboxes
and progress counts in task details. Active
and Archive tabs separate unfinished and completed tasks; archive details allow
only restoration. It uses a
same-origin development API proxy to the Rust backend. The Telegram SDK bridge
provides signed authorization data and theme support. Browser launches without
Telegram credentials show sign-in guidance. Frontend architecture, commands, and testing
rules live in `frontend/AGENTS.md`; local setup is in `frontend/README.md`.

Rust, React, TypeScript, Vite, Material UI, npm, PostgreSQL, Axum, Tokio, and SQLx are decided.
Telegram Mini App authentication is implemented; standalone browser login,
deployment, and schemas for future features remain undecided.
Outbound Telegram bot messaging is implemented for daily-list finalization,
using a shared reqwest client inside the backend. A background worker preserves
unfinished expired picks, offers them at 08:00 in their saved device timezone,
and polls Telegram Yes/No callbacks. PostgreSQL stores offers and polling progress;
private-chat `/today` resends the current daily list, while `/start` and `/help`
explain commands. The worker registers Telegram command suggestions at startup
and polls messages alongside callbacks. Commands reuse user-scoped daily logic.
An advisory lock selects one worker. Subcategories and implementation
plans are not implemented yet.

Morning suggestions and informational reminder notes share one message per offer
when they fit Telegram's size limit, with the selection question and Yes/No
buttons at the end. `/morning` resends due, unexpired, unanswered offers with live
filtering and original decision IDs; it never reopens answered offers or selects
tasks automatically. Empty `/today` replies point users to `/morning`.

Private-chat voice recordings are transcribed through a provider-neutral
`SpeechRecognizer`. OpenAI is the initial provider, implemented
as a custom `langchain-rust` Tool using `gpt-4o-mini-transcribe`; startup requires
backend-only `OPENAI_KEY`. Telegram Ogg/Opus voice files stay in memory (20 MB
maximum), and replies are HTML-escaped and split. A provider-neutral
`VoiceAssistant` uses a bounded `langchain-rust` tool-calling conversation with
OpenAI `gpt-4o-mini` and the same key. Tools list all current daily work with
namespaced IDs, then complete an explicitly requested listed task/subtask, and
also list categories and active tasks, then create an explicitly requested
ordinary task with optional ordered steps or append one step to a listed active
task. Creation accepts only IDs returned by this message's listings; new steps
start unchecked and never complete the parent.
Ordinary work, accepted scheduled occurrences, and selected reminders are supported.
Writes recheck ownership, daily membership, expiry, and descriptions under locks;
ordinary subtask completion preserves stable IDs and auto-archives a fully completed
parent. Ambiguous matches request a new recording with full name/category.
After a successful voice completion, the bot sends the live, shared daily summary
with updated completion marks. Summary-load or delivery failure preserves progress;
`/today` retries the summary without repeating the mutation.
Each recording is independent and can complete or create one item; unrelated speech is echoed.
 Non-command private-chat text enters the same assistant flow directly, as if
already transcribed, with identical completion and creation guards, replies, and
daily summaries.
Each text message is independent too; blank text, unknown slash commands, and
addressed `@bot` commands are ignored. Polling offsets commit before assistant
side effects so delivery failures do not replay mutations.
Example requests: “What are my tasks for today?”, “Mark buy groceries completed.”,
“I finished the deploy backend step in Release, category Work.”, or “Add a task
buy groceries to Home with steps milk and bread.” Transcripts are not persisted.
See the backend guide for integration details.

After the 08:00 setup, the bot reminds users of unfinished ordinary tasks outside
Tasks of the day, ordered Soon → Upcoming → Someday. A Yes/No offer waits for the
user's decision; mornings without an actionable offer send the reminder directly.
Any active pick for a task (including one subtask) excludes that task. Scheduled
and reminder tasks never appear. `/remind` sends this live backlog on demand;
`/start` and `/help` explain both `/today` and `/remind`. Morning backlog delivery
uses saved timezone history, falling back to UTC when none exists, and persists
delivery/retry state across restarts.

Scheduled tasks live in separate user-owned `scheduled_tasks` and
`scheduled_occurrences` tables. The Scheduled tasks tab manages description,
optional ordered subtasks, and a required weekly schedule (one or more
weekdays; all seven means daily). Saved device timezone determines planning days,
which run from local 02:00 to the next local 02:00. A background sweep and lazy
API generation create today's due candidates idempotently, including after
downtime, without backfilling missed days. At local 08:00 the bot offers them
alongside ordinary carryover (or on their own). Yes adds them to the daily list;
No leaves them out. Each accepted occurrence starts unchecked;
completion and removal affect today only. Schedule edits affect future snapshots;
deleting a schedule deletes its occurrences. Only accepted occurrences join daily-list
Telegram summaries. Scheduled work is offered fresh on due days, never carried
over from an expired occurrence.

Bot messages linking into a specific Mini App category/task were discussed as
a possible next feature. Destination links and launch-destination handling are
not implemented; no deep-link format has been adopted yet. Backend would send
the destination, frontend would expand/focus it, and existing user ownership
checks would still apply. See the subproject guides for integration options.

Reminder tasks have their own UI tab and user-owned `reminder_tasks` table.
Reminder and Scheduled tabs have per-task **Add to today** buttons for unfinished
work due today but absent from the daily list. Manual selection works independently
of morning offers, is idempotent, and restores removed scheduled occurrences with
their existing progress. Saved timezone determines eligibility: reminder calendar
date, or scheduled planning day starting at 02:00. Original expiry rules apply.
The saved timezone defines calendar dates: at 08:00 on the preceding date the
bot lists tomorrow's reminders (informational only); at 08:00 on the event date
it offers them alongside scheduled work and carryover with the existing Yes/No
buttons. Notices also send without other daily work. Exact time appears in bot
messages and UI. Acceptance adds reminders until the following calendar day's
02:00; completion updates the reminder and daily expiry/removal preserves it.
Reminders never recur or carry over. Rescheduling clears daily selection and old
buttons cannot select changed dates/timezones. Completed reminders remain in their
own tab and can be reopened. No separate event-time alert is sent.
Reminder selection expires at the following calendar day's 02:00 in its saved
timezone, whether completed or not. The next 08:00 morning message includes an
informational note naming yesterday's unfinished reminders, without offering to
carry them over. The note also sends when no tasks need selection; completed,
deleted, and rescheduled reminders are filtered before delivery.

## Container images and runtime

Alpine-based multi-stage Dockerfiles live in each subproject. Separate GitHub
Actions workflows publish frontend/backend images to this repository's GHCR
packages on pushes to `main` changing `frontend/**` or `backend/**`, respectively.
Both images target `linux/amd64` and `linux/arm64`, with `latest` and
`sha-<full-commit-sha>` tags:

| Changed paths | Image |
| --- | --- |
| `backend/**` | `ghcr.io/blackmorse/life-organizer/backend` |
| `frontend/**` | `ghcr.io/blackmorse/life-organizer/frontend` |

Workflows authenticate with the built-in `GITHUB_TOKEN` using `packages: write`;
no registry secret is needed. All build and runtime stages use Alpine-based images.
Backend images build in parallel on native `ubuntu-latest` (AMD64) and
`ubuntu-24.04-arm` (ARM64) runners, with architecture-specific BuildKit caches.
Each build pushes by digest; a dependent job merges both digests into one
multi-architecture manifest tagged `latest` and `sha-<full-commit-sha>` only after
both builds succeed. Backend workflow edits also trigger publication.

Build locally from the repository root:

```sh
docker build -t life-organizer-backend ./backend
docker build -t life-organizer-frontend ./frontend
```

The backend listens on `0.0.0.0:3000` in its container and requires runtime
`DATABASE_URL`, `TELEGRAM_BOT_TOKEN`, and `OPENAI_KEY` environment variables.
PostgreSQL runs separately. Database migrations are embedded in the binary and
applied at startup. Credentials are not needed during image builds.

The frontend serves HTTP on port 80 and proxies `/api/*` to
`http://backend:3000/*`, stripping `/api`. Put both containers on the same Docker
network and name the backend container/service `backend`, or set the frontend's
`BACKEND_URL` environment variable to another reachable backend origin (without
a trailing slash). Configure HTTPS at the public ingress for Telegram Mini App
hosting.

## Guidance for future changes

- Keep the root `README.md` focused on a high-level, human-readable description
  of the app's purpose and capabilities. Put implementation details, migration
  notes, and operational guidance in the appropriate `AGENTS.md` guide.

Tasks of the day are implemented: users select whole active tasks or individual
subtasks with calendar buttons, then track completion in a dedicated tab.
Separate calendar **+1** buttons plan ordinary tasks/subtasks for tomorrow without
changing today's picks. Parent selection uses all current steps, as for today.
Tomorrow means the next calendar date in the device IANA timezone captured on add,
even before 02:00. Migration `0014_tomorrow_tasks.sql` stores independent picks;
at the target date's 02:00 they move into durable morning-offer storage. At 08:00
they join unfinished carryover, scheduled work, and reminders in the same Yes/No
offer, including mornings with only planned work. Yes adds still-unfinished owned
work to today's list without duplicates; No dismisses. Completed/deleted tasks or
steps are filtered live, edits/reorders preserve stable links, and removing a
future pick changes no original work. Restart catches up still-valid offers only.
Selections persist in PostgreSQL and expire at the next 02:00 in the device's
IANA timezone supplied when adding each pick. Adding a task with subtasks selects
all its current steps instead of the parent, preserving completion flags; tasks
without steps remain whole-task picks. The parent calendar removes all its picks
when fully selected. Bot lists group work under 📁 categories with nested tasks
and subtasks; category-free scheduled work uses a 🔁 Scheduled section.
Expiry removes selections only;
original work and progress remain. Selected subtasks receive stable, task-owned
IDs so edits and reordering preserve their daily links.
The daily tab's Finalize tasks button sends the current server-resolved list to
the authenticated user's private bot chat. Finalization does not lock or complete
tasks; users can edit and explicitly resend. Users must start/unblock the bot.
The Mini App also resends the current summary after saved completion changes
affect daily picks. Delivery failure preserves saved progress and allows manual
resending through Finalize tasks.
Daily bot reports end with completed/total selected work and a ten-cell text
progress bar with percentage. Selected tasks and subtasks each count once;
context-only parent headings do not count. Finalization, `/today`, and completion
reports share this footer, shown once after the full list.
At 08:00, the bot offers unfinished picks from the previous daily list with Yes/No
buttons. Yes restores only still-unfinished owned work, without duplicating current
picks; No dismisses the offer. Offers expire at the next local 02:00. The backend
must be running; after downtime it sends still-valid due offers. Bot-restored picks
appear when reopening or refreshing the Mini App.

- Preserve the two-subproject monorepo structure unless explicitly asked to change it.
- Keep the bot within the backend initially.
- Keep domain logic independent of UI and Telegram transport details.
- Split independent backend responsibilities into focused modules.
- Cover backend behavior with tests. End-to-end tests are primarily human-maintained;
  agents must not change that suite or its supporting artifacts without an explicit
  human request specifically authorizing end-to-end test work. See `backend/AGENTS.md`.
- Update this guide when architecture decisions or repository layout change.
- Add build, run, and test instructions once the relevant subprojects and tooling exist.
