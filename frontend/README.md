# Scheduler frontend

React + TypeScript + Vite + Material UI app for the Rust backend in `../backend`.

## Features

- Browse an expandable category/task hierarchy, with per-category counts.
- Use one calendar button beside an active task or subtask to open **Add for today**,
  **Add for tomorrow**, or **Select date**. Today and tomorrow options become removal
  actions when already selected. Select date opens a future-date picker; the bot
  offers unfinished work at **8:00 AM on that date** in your saved device timezone.
  Choosing a new future date replaces that item's previous future plan. A parent with
  subtasks selects all its current steps; today and tomorrow selections are
  independent and survive reloads. Tomorrow means the next calendar date in your
  saved device timezone, including when planning before 2:00 AM. At tomorrow's
  **8:00 AM**, the bot includes unfinished planned work in its usual **Yes/No**
  plan offer alongside carryover, scheduled tasks, and reminders. Planned-only
  mornings get an offer too. **Yes** adds eligible work to today's list without
  duplicates; **No** skips it. Completed/deleted work is excluded. Reopen or
  **Refresh** to load accepted picks. Future highlights clear at the target day's
  **2:00 AM**, when picks enter morning-offer storage. Backend must be running;
  restart catches up still-valid offers. Restart to apply `0014_tomorrow_tasks.sql`.
- **Reminder tasks** manages one-time tasks with a required date and optional time,
  without category or urgency. Leave **Time (optional)** blank for work to do anytime
  during that day; lists and bot messages show **All day**. Enter date/time in your device timezone; reminders retain
  that saved timezone. At **8:00 AM on the preceding calendar date**, the bot lists
  tomorrow's reminders with their times or **All day**. At **8:00 AM on the event date**, it offers
  to include them in **Tasks of the day** alongside other morning suggestions.
  **Yes** adds today's reminders; **No** leaves them out. Tomorrow's notice is
  informational only. Messages also send when no other work is due. No additional
  alert is sent at the event time. Dates follow calendar days even for events before
  2:00 AM. Accepted reminders show a **Reminder** badge and saved-zone date with optional time;
  progress is saved to the original and included under **⏰ Reminders** in bot lists.
  Daily picks expire at **2:00 AM the next calendar day** without deleting reminders
  or progress. Reminders never repeat or carry over. Unfinished past reminders remain
  in their tab; completed reminders are deleted at 2:00 AM after their due date.
  Until then they can be reopened. Rescheduling or changing time mode clears daily selection
  and invalidates old buttons; completion stays saved. The backend must be running;
  it catches up still-valid morning groups after downtime. Reminders created after
  their morning group was sent/answered do not join that offer. Restart backend to
  apply migrations, including `0019_reminder_optional_time.sql`; existing timed
  reminders retain their times. The open app refreshes accepted work every 30 seconds
  and on focus, or use **Refresh**.
- **Scheduled tasks** manages recurring tasks without categories. Create a
  description, choose at least one weekday (e.g. Monday, Wednesday, Friday),
  and save the schedule. Select all seven days for daily repetition. Device
  timezone is prefilled; schedules keep that saved IANA timezone. Optional
  ordered subtasks are supported; scheduled tasks have no urgency.
  On due days, the bot offers a fresh unchecked occurrence at **8:00 AM** in the
  schedule timezone. Press **Yes** to add it to **Tasks of the day**, marked
  **Scheduled**, or **No** to skip. Nothing is added automatically. Scheduled-only
  days get an offer too; work sharing the same timezone/day joins the carryover offer.
  Complete or skip an accepted occurrence without stopping future repeats.
  Open the daily occurrence to track/edit its subtasks; completing every step
  completes today's occurrence only. Schedule edits affect future occurrences;
  deleting a schedule removes its occurrences too. No category is needed.
  Planning days start at **2:00 AM** in the saved timezone, so before that boundary
  the previous weekday still applies. The backend generates due work while the app
  is closed and catches up today's work after downtime, without adding missed days.
  The open app reloads scheduled occurrences every 30 seconds and on focus,
  clearing expired rows at rollover even if the network is unavailable.
  Only accepted scheduled occurrences join finalized Telegram summaries and progress
  updates. Expired occurrences never carry over; future due days get fresh offers.
  Schedules created after an offer was sent or answered wait for their next due day.
  Restart the backend to apply migration `0011_scheduled_morning_offers.sql`
  automatically. Existing daily occurrences and progress stay visible; new days
  require accepting the offer. Reopen or **Refresh** to load accepted work.
- Use the calendar menu beside active tasks or subtasks to add/remove daily picks.
  Adding a task with subtasks selects **all its current subtasks** instead of the
  parent, preserving completed flags. The parent's Today option fills a partial
  selection or removes all steps when fully selected. Tasks without subtasks are picked whole.
  **Tasks of the day** shows a checklist linked to the original work. Selected
  subtasks appear indented beneath their bold parent task name, grouped
  together per task. Whole-task picks keep their own checkbox and remove button.
  Completing items updates the original task/subtask; finished picks stay visible for the day.
  Remove buttons only remove picks. The list clears at the next **2:00 AM** in
  your device timezone, preserving tasks and progress. Picks persist across reloads
  and expire even while the app is closed. Timezone is captured when adding each
  pick; later timezone changes do not move existing expiry times.
- At **8:00 AM** in that saved device timezone, the bot offers unfinished work
  from your previous daily list. Press **Yes** to add it to today's list, or **No**
  to skip. Completed/deleted work is excluded, and existing picks are not duplicated.
  Offers expire at the next **2:00 AM**. Reopen or **Refresh** the app to see restored
  picks. The backend must be running; after downtime, still-valid offers are sent.
- In **Tasks of the day**, press **Finalize tasks** to receive the current list
  from your bot in Telegram. The HTML message groups work under bold **📁 category**
  headings, with indented bold task names and subtasks indented one level deeper.
  Category-free scheduled work uses **🔁 Scheduled**, with its occurrence subtasks.
  Morning offers use the same hierarchy. 🔹 marks unfinished picks; ✅ marks completed
  picks. 📋 marks context-only parent names above selected steps. Search does not limit what is
  sent. Your list stays editable; pressing Finalize again sends an updated list.
  Marking or unmarking daily work automatically sends a fresh summary after progress
  saves, including changes made from other tabs. Failed delivery keeps saved progress
  and shows guidance; use **Finalize tasks** to resend after checking your bot chat.
  Long lists arrive as multiple messages. Delivery failures leave picks intact;
  if delivery is uncertain, check the bot chat before retrying to avoid duplicates.
- Send **`/today`** in the bot's private chat to receive your current daily list
  again, including accepted scheduled tasks and saved progress. **`/start`** and
  **`/help`** explain available commands. The backend registers these in Telegram's
  command suggestions when it starts; restart the backend to enable the commands.
- Complete tasks from expanded details to move them out of the **Active** tab
  into **Archive**. Archive keeps details and subtasks readable; **Restore** is
  its only task action. Restore before editing, moving, or deleting a task.
  Counts and search reflect the selected tab.
- Open task rows to read full details; collapse branches or expand/collapse all.
- Search category names, task descriptions, and subtasks; matching branches expand automatically.
- Create, rename, and delete categories through on-demand forms and action menus.
- Create, edit, move, and delete tasks without keeping creation forms on screen.
- Choose task urgency: **Someday** (at some point in the future), **Upcoming**
  (next few weeks), or **Soon** (next few days). Labels appear on task rows;
  create/edit forms let you change them.
- Loading/saving indicators, refresh, empty states, and API errors.
- Failed writes preserve form contents for retry.
- Break tasks into ordered subtasks in expanded details. **Add subtask** below the
  list opens a focused text input. Use each row's pencil to edit, cross to delete,
  or arrows to reorder. Add/edit forms offer **Save subtask** and **Cancel**;
  delete and reorder save immediately. Failed saves retain typed text or the
  existing list. Task rows show completed/total step counts.
- Check or uncheck subtasks directly in task details. Completed steps stay visible.
  Completing every step in a nonempty list automatically moves the parent task
  to Archive. Archive checkboxes are read-only; Restore keeps progress so you can
  reopen individual steps.
- Material UI components, responsive layout, light/dark themes, and an optional
  Telegram SDK bridge with live theme updates.
- Personal workspaces using verified Telegram Mini App authorization. Each user
  sees only their own categories and tasks.

Ordinary tasks have descriptions, categories, and urgency. New and existing tasks default
to Someday; restarting the backend applies the additive urgency migration
automatically. These are planning horizons, not enforced due dates.
Subcategories and implementation plans are not part of the current backend contract.
Subtasks are optional ordered descriptions with completion flags, without their
own urgency or archive. An empty list does not auto-complete a task.
Restarting the backend applies the additive subtask migration automatically;
existing tasks start with empty lists. Task edits and category moves preserve steps.
Completion also preserves task content, urgency, category, and steps. Restarting
the backend applies migration `0005_task_completion.sql`; existing tasks start
active. Categories containing archived tasks cannot be deleted until those tasks
are restored and moved or deleted.
Migration `0006_subtask_completion.sql` converts existing subtask text to objects
with `completed: false`, preserving descriptions, order, and parent completion.
It runs automatically on backend restart. API subtasks are now objects such as
`{ "description": "Build UI", "completed": false }` rather than plain strings.
Migration `0007_daily_tasks.sql` adds persistent daily picks on backend restart.
Selected steps gain stable optional string IDs; edits and reordering keep daily
picks linked to the same step, and deleting a step removes its pick.

## Local development

Use Node 24.15.0 or newer within Node 24 (see `.nvmrc` and `package.json`).
With nvm installed, run `nvm install` and `nvm use` from this directory.

Start the Rust backend in a separate terminal, from `backend/`, using an existing
PostgreSQL database:

```sh
DATABASE_URL=postgres://user:password@localhost:5432/scheduler \
TELEGRAM_BOT_TOKEN='<your BotFather token>' \
OPENAI_KEY='<your OpenAI API key>' cargo run
```

Then, from `frontend/`:

```sh
npm ci
npm run dev
```

Vite binds to **http://127.0.0.1:5173**, on loopback with a strict port. Opening
that URL directly shows Telegram sign-in guidance; use the Mini App setup below
to access private data. Standalone browser login is not implemented.

The ownership migration assumes empty categories. Clear old shared tasks and
categories before restarting the backend with this version. Migrations run
automatically; they do not assign legacy data to an arbitrary user.

The browser requests `/api/categories` and `/api/tasks`. Vite forwards `/api/*`
to `http://127.0.0.1:3000/*`, removing `/api`. No backend CORS change is needed.
All app data comes from PostgreSQL through the API; the frontend has no mock
data or local-storage fallback in normal use.

If the backend uses another address, create `frontend/.env.local`:

```dotenv
API_PROXY_TARGET=http://127.0.0.1:3001
```

Restart Vite after changing environment configuration. See `.env.example` for
available variables. No bot token belongs in frontend environment variables.

## Run as a Telegram Mini App against the local backend

`index.html` loads Telegram's official Web App SDK. The adapter calls
`ready()` and `expand()` after mounting. The Material UI theme reads Telegram colors, reacts to theme
changes, and falls back to browser light/dark preferences. Layout respects
Telegram content safe-area insets.

1. Create/select a bot in **@BotFather**. Start the local backend using that bot's
   token as `TELEGRAM_BOT_TOKEN` (see command above). Keep the token backend-only.
2. Start an HTTPS tunnel to Vite. For example, with Cloudflare Tunnel installed:
   ```sh
   cloudflared tunnel --url http://127.0.0.1:5173
   ```
3. Copy the generated hostname into `frontend/.env.local`:
   ```dotenv
   DEV_ALLOWED_HOST=random-name.trycloudflare.com
   API_PROXY_TARGET=http://127.0.0.1:3000
   ```
   Use the actual hostname, without `https://` or a path. Restart Vite after
   changing it. Keep host validation enabled.
4. In BotFather, send `/setmenubutton`, select the same bot, choose button text,
   and enter the full HTTPS tunnel URL.
5. Open the bot's private chat and tap its menu button. No bot polling process
   or webhook is needed for this launch flow. Keep PostgreSQL, backend, Vite,
   and the tunnel running. A new quick tunnel URL requires updating both
    BotFather and `DEV_ALLOWED_HOST`.

To receive finalized daily lists, first press **Start** or send `/start` in that
bot's private chat and ensure the bot is unblocked. The backend sends messages
with the same `TELEGRAM_BOT_TOKEN` used for Mini App authorization; no additional
bot service or chat-ID configuration is required. The backend runs its morning
reminder worker and Telegram callback polling automatically. Keep this bot's
webhook disabled and avoid a second poller using the same token.

The app sends `Authorization: tma <raw Telegram.WebApp.initData>` on every API
request. The backend verifies the HMAC signature and freshness, registers the
signed Telegram user automatically, and returns only their data. Client-supplied
user IDs and `initDataUnsafe` are not used for authorization. Credentials are not
stored in local storage or sent in URLs. The tunnel exposes Vite and its API
proxy; API access requires valid Telegram authorization.

Missing credentials or invalid/expired authorization shows sign-in guidance.
Close and reopen the Mini App after one hour to obtain fresh initData. A browser
tab or unauthenticated `curl` request cannot access the workspace; protected API
routes return `401`. `GET /api/me` returns the authenticated internal `id` and
`telegram_id`. Bot tokens must never be sent to the frontend.

## Checks and build

```sh
npm run lint
npm test
npm run build
npm run preview
```

- `npm test`: Vitest + React Testing Library component tests in jsdom; fetch is
  mocked at the HTTP boundary. No Docker or running backend is required.
- `npm run build`: TypeScript checks and optimized assets in `dist/`.
- `npm run preview`: local build preview, with the same API proxy configuration.
  It is not a production deployment server.

Production hosting must serve `dist/` and route `/api/*` to the Rust backend with
the prefix removed. Vite's development proxy is not included in static assets.

See `AGENTS.md` for module boundaries and contribution rules.
