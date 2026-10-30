# Frontend Guide

## Purpose and stack

This subproject provides the personal task-organizing UI for the Rust API in
`../backend`. It targets Telegram Mini App use while remaining usable in a
standalone browser.

- React 19, TypeScript with strict checks, Vite 8, npm.
- Node 24 via `.nvmrc`; minimum 24.15.0, as declared in `package.json`.
- Oxlint for linting; Vitest, React Testing Library, and jsdom for component tests.
- Material UI 9 with Emotion and Material icons. No router or global state library yet.
- Commit `package-lock.json`; use `npm ci` for reproducible installs.

## Current functionality

- Scheduled tasks tab manages category-free recurring tasks with description,
  ordered subtask templates, required weekday selection, and saved IANA
  timezone (initially the device timezone). Select all weekdays for every day.
  Due occurrences are offered by the bot at 08:00 in their saved timezone, even
  without ordinary carryover. Yes adds them to Tasks of the day with a Scheduled
  badge; No leaves them out. Reads never automatically select scheduled work.
  Completion, subtask edits, and removal affect that occurrence only; removing
  skips today without regenerating on refresh. Template edits affect future
  snapshots; deletion removes the schedule and its occurrences. No template archive.
  Planning days follow saved timezone, local 02:00 to next 02:00, including DST.
  Current scheduled occurrences refresh every 30 seconds while visible and on focus;
  an independent timer clears expired rows at rollover even if a request hangs.
  Backend generation also runs while the app is closed and catches
  up current-day candidates after downtime. No missed-day backfill or scheduled
  carryover; each due day gets fresh candidates. Reopen/Refresh loads accepted work.
  Scheduled progress changes resend the current Telegram summary after saving.
- Tasks of the day tab tracks whole-task and individual-subtask picks with
  completion checkboxes and remove buttons. Calendar buttons beside active task
  rows and subtask rows toggle selection. Adding a task with subtasks selects all
  its current steps instead of the parent, preserving completed flags. Its parent
  calendar is selected when all steps are picked; clicking it removes all picks
  for that task. Partial selections can be filled by adding the parent again.
  Tasks without subtasks remain whole-task picks. Selected subtasks are grouped by task ID
  beneath a single bold parent task name in an indented nested list. Context-only
  parents have no completion/remove controls; selected whole tasks keep theirs.
  Picks persist through the backend;
  completion updates originals and completed picks remain until removal/reset.
  Each pick expires at the next 02:00 in the device IANA timezone captured on add.
  A timer and focus/visibility checks clear expired UI picks, including on resume.
  Original tasks and progress are never cleared by daily expiry.
  At 08:00 in the saved device timezone, the backend bot offers unfinished picks
  from the previous daily list with Yes/No buttons. Yes restores still-eligible
  picks until next local 02:00; No dismisses. Reopen/Refresh loads bot-restored picks.
- Finalize tasks sends all current daily picks to the user's private bot chat,
  grouped under bold 📁 category headings, with indented tasks and deeper-indented
  subtasks. Category-free scheduled work uses a 🔁 Scheduled section.
  independent of the search filter. Empty lists and in-flight writes disable the
  button. Success appears only after confirmed API success; errors retain picks
  and surface Start/unblock or partial-delivery guidance. Finalization does not
  lock the list; explicit subsequent clicks resend the current list.
  Successful task/subtask completion changes affecting an unexpired daily pick
  automatically resend the full server-resolved summary, including reopening and
  parent auto-completion, from any tab. Writes and sends remain serialized.
  Failed completion saves never send. Delivery failure keeps saved progress and
  shows separate delivery/retry guidance; Finalize tasks can resend manually.
  Users can also send `/today` in the bot's private chat to fetch the same current
  list; `/start` and `/help` explain commands. Backend registers command suggestions.

- Read-first category/task hierarchy, with compact rows and expandable details.
- Active and Archive tabs. Complete moves a task into the archive; Restore returns
  it to active tasks with its category, urgency, and subtasks intact. Archive has
  read-only task/subtask details and no task/category editing or creation controls.
  Counts/search apply only to the selected tab; empty archive categories are hidden.
- Category CRUD and task counts. Categories start expanded; users can collapse
  individual branches or expand/collapse all categories.
- Search across category names, task descriptions, and subtask text. Matching branches expand
  during search; clearing it restores the previous collapse state.
- Task CRUD and moving tasks between categories. Creation forms appear only
  after explicit add actions. Task edit/delete actions live in expanded details;
  category rename/delete actions use Material UI menus.
- Task urgency: Someday (future), Upcoming (next few weeks), Soon (next few days).
  Create/edit forms select urgency; compact rows show a labeled badge and details
  explain the planning horizon. New tasks default to Someday.
- Required descriptions, category selection, loading/saving states, API errors,
  and failed-write draft retention.
- Ordered subtasks in expanded task details. Add subtask below the list opens a
  focused text input immediately; a pencil on each row edits its description
  inline. Save subtask saves the change; Cancel discards it. Failed saves preserve
  typed text. Each row has a cross for immediate deletion and up/down reorder
  controls. Other list mutations are disabled while an add/edit draft is open.
- Subtask completion checkboxes save immediately in read view, with progress counts
  and completed steps still visible. Completion flags stay with their steps when
  descriptions change or steps are reordered. A nonempty fully completed list automatically
  archives the parent using the returned backend task. Empty lists stay active.
  Archive checkboxes are disabled; Restore preserves progress so steps can be reopened.
- Backend remains authoritative for validation, relationships, and persistence.
  Nonempty category deletion produces a conflict; tasks are not cascade-deleted.
- Subcategories and implementation plans do not exist yet.
- Telegram bridge initialization, theme support, and Mini App authorization are
  present. Each API request sends raw signed `initData`; the backend verifies it
  and scopes data to that user. Browser launches without initData show sign-in
  guidance. Standalone browser authentication is not implemented.
- Daily-list bot messaging is implemented through the backend. Public hosting and
  deployment are not implemented.

## Module boundaries

| Path | Responsibility |
| --- | --- |
| `src/main.tsx` | React root and Material UI theme provider. |
| `src/App.tsx` | Read-first workspace layout, search, and on-demand category creation. |
| `src/types.ts` | API entity/input types matching backend JSON. |
| `src/urgency.ts` | Central urgency values, labels, planning horizons, and badge colors. |
| `src/api.ts` | Fetch client, JSON requests, empty delete responses, error handling. |
| `src/hooks/useWorkspace.ts` | Initial loading, refresh, serialized mutations, authoritative response state. |
| `src/components/CategoryTree.tsx` | Expandable category/task hierarchy, search filtering, and category actions. |
| `src/components/CategoryForm.tsx` | Shared on-demand category create/rename form. |
| `src/components/TaskForm.tsx` | Shared create/edit form with category and urgency selection. |
| `src/components/TaskCard.tsx` | Compact task row, expandable read view, and edit/move/delete actions. |
| `src/components/Subtasks.tsx` | Numbered checklist, progress, direct add, and per-row inline edit/delete/reorder actions. |
| `src/components/DailyTasks.tsx`, `DailyContext.ts` | Daily checklist, calendar pick controls, and shared selection actions. |
| `src/components/ScheduledTasks.tsx`, `ScheduledDaily.tsx` | Recurring-template forms/list and independent daily-occurrence progress. |
| `src/telegram.ts` | Optional, isolated Telegram SDK bridge. |
| `src/theme.tsx` | Material UI palette, typography, defaults, CssBaseline, and live Telegram/OS theme support. |
| `src/App.test.tsx`, `src/test/` | Component-level behavior tests and shared setup. |
| `vite.config.ts` | React plugin, dev/preview API proxies, host configuration, Vitest. |

Keep fetch and error translation in the API module. Keep Telegram globals behind
the adapter rather than spreading SDK calls through components. Do not put
database access, bot tokens, or backend business rules in frontend code.

Use Material UI components for controls, menus, forms, and layout. Put shared
visual defaults in the theme and use theme-aware `sx` for local layout. Avoid
parallel handwritten CSS or hand-built replacements for existing MUI controls.
Keep the initial screen focused on reading; do not restore always-visible
creation forms. The hierarchy uses semantic nested lists and disclosure buttons
with `aria-expanded`, not ARIA tree roles that would require tree keyboard navigation.

The workspace currently loads all categories/tasks, searches locally, and updates
state from successful mutation responses. Refresh reloads server state; cancelled
or stale loads must not overwrite newer data. Failed writes must not clear forms.
Keep mutation controls disabled while loading/saving to prevent duplicate writes.

Use visible labels, keyboard-accessible buttons, announced errors/status, and
mobile-friendly layouts. Retain standalone-browser behavior when adding Telegram
features. Do not invent API fields absent from the backend contract.

## Bot-to-Mini-App navigation (discussed, not implemented)

Bot messages could open a category/task directly. With a Main Mini App
configured in BotFather, a Telegram link can carry `startapp=category_42`;
Telegram exposes it as `start_param` in launch data and `tgWebAppStartParam`
in the URL. A private-chat `web_app` button can instead supply an ordinary app
URL query parameter. These are options; no destination format is adopted yet.

Future handling should parse navigation hints behind the Telegram adapter,
load the authenticated workspace, then expand the target category, scroll to
it, and optionally open task details. Missing/inaccessible destinations should
show an unavailable state. Launch parameters do not grant access or establish
identity; backend verification of raw signed initData remains authoritative.

## API and development configuration

Browser URLs are same-origin `/api/*`. Dev and preview proxies strip `/api` and
forward to `API_PROXY_TARGET` (default `http://127.0.0.1:3000`). Backend routes
remain `/categories` and `/tasks`, including `/{id}` for read/update/delete.

- Create: POST; update: PUT; delete: DELETE returning 204.
- Category input: `{ "description": "Work" }`.
- Task input: `{ "category_id": 1, "description": "Build UI", "urgency": "upcoming" }`.
- Task responses include `completed: boolean` (false for new tasks). The list API
  returns active and archived tasks together; the UI partitions them locally.
  `PUT /tasks/{id}/completion` accepts `{ "completed": true }` to archive or
  `{ "completed": false }` to restore, returning the updated task. Archived task
  edits, moves, subtask writes, and deletion return `409`; restore is the only
  allowed task write. Nonempty category deletion counts archived tasks too.
  Failed completion/restore requests leave tasks in their current tab for retry.
- Task responses include `subtasks: { description: string, completed: boolean }[]`
  in saved order. New tasks have an empty list. `PUT /tasks/{id}/subtasks` accepts
  `{ "subtasks": [{ "description": "Design", "completed": false }] }` and returns
  the updated task, including its derived completion state. An empty list clears
  steps without completing the parent. Saving a nonempty fully completed list
  archives the parent atomically. Ordinary task updates/moves preserve steps and
   flags. Steps may include an optional stable string `id`, assigned on first
   daily selection. Preserve returned IDs on all list writes so daily picks
   follow edits/reorders. Steps have no independent archive or nested children; parent task
  deletion removes its steps. List writes remain atomic and last-write-wins.
  Checkbox/delete/reorder failures keep the server-confirmed list; failed inline
  add/edit saves retain typed descriptions. Description edits preserve completion.
- Task responses include urgency (`someday`, `upcoming`, or `soon`). Always send
  urgency on writes, including task moves; omitted API input defaults to Someday.
  Extend the catalog in `src/urgency.ts` alongside the backend enum and a new
  database constraint migration. These horizons do not create due dates.
- IDs are generated by the backend; never use descriptions as keys.
- `/scheduled-tasks` GET/POST and `/scheduled-tasks/{id}` PUT/DELETE manage
  templates. Input: `{description, weekdays, timezone, subtasks}`;
  responses add `id`. Weekdays are distinct ISO numbers 1 (Monday) through 7
  (Sunday), with at least one required. No category, urgency, or step IDs. Failed
  saves retain drafts. Save schedule stays disabled while editing a subtask draft.
- `GET /scheduled-tasks/daily` returns only accepted occurrences as `{id, scheduled_task_id, description,
  subtasks, completed, expires_at}[]`. Progress endpoints are
  `PUT /scheduled-tasks/daily/{id}/completion` with `{completed}` and
  `PUT /scheduled-tasks/daily/{id}/subtasks` with `{subtasks}`. DELETE on
  `/scheduled-tasks/daily/{id}` skips today only. All return the current occurrence
  list. Expiry is Unix milliseconds. Future days start unchecked even if today's
  occurrence was completed or skipped. Scheduled and ordinary IDs are separate
  namespaces; never route scheduled progress through ordinary task APIs.
- `GET /daily-tasks` returns `{task_id, subtask_id, expires_at}[]` (expiry in Unix
  milliseconds; empty subtask ID means whole task). `POST /daily-tasks` accepts
  `{task_id, timezone, subtask_index?}` and `DELETE /daily-tasks` accepts
  `{task_id, subtask_id}`; both return the current picks. Omitted `subtask_index`
  selects every current subtask, or the whole task when none exist. Empty
  `subtask_id` on DELETE removes all picks for that task. Adding steps may assign
  its stable ID, so the hook reloads tasks before publishing picks. Expiry is
  server-enforced; changing device timezone does not alter existing picks' expiry.
- `POST /daily-tasks/finalize` has no body and returns `204` after Telegram accepts
  the summary. The backend derives recipient and list from verified identity and
  stored picks. Empty lists/unavailable bot chats return `400`; other Telegram
  delivery errors return sanitized `502` text preserved by the client for retry
  guidance. No task completion, expiry, or selection state changes on finalize.
- Service errors return `{ "error": "..." }`; extractor failures can be text.
- Vite starts on `127.0.0.1:5173`, with `strictPort` enabled.
- `.env.local` can override the proxy target and exact `DEV_ALLOWED_HOST` for a
  tunnel. Never disable host checks globally or store credentials in `VITE_*`.
- A public tunnel makes the API proxy public. Backend authentication and access
  control use Telegram initData; host validation is not authentication.
- The client sends `Authorization: tma <raw initData>` on every request. Never
  trust `initDataUnsafe`, send a client-selected user ID, save initData in browser
  storage, or put credentials in URLs. Bot tokens remain backend-only.
- Missing initData blocks API requests and shows Telegram launch guidance.
  `401` responses hide the workspace and ask users to close/reopen the Mini App.
  InitData expires after one hour; the SDK does not refresh it in place.
- Static production hosting needs its own `/api` reverse proxy; Vite config is
  not embedded into the built client.

## Commands

From `frontend/`, using Node 24:

```sh
npm ci
npm run dev
npm run lint
npm test
npm run test:watch
npm run build
npm run preview
```

Start the backend separately with `DATABASE_URL` configured; see
`../backend/AGENTS.md`. Full local setup is in `README.md`.

## Verification and testing boundaries

Run lint, component tests, and production build for UI or client changes.
Component tests exercise the real app/client with mocked HTTP responses; they
do not verify PostgreSQL, Telegram WebViews, CSS layout, or a live backend.
Cover meaningful CRUD interactions and error recovery rather than snapshots of
implementation details. Backend contract changes need coordinated updates.

End-to-end tests and supporting artifacts remain human-maintained. Do not
create, modify, delete, disable, or regenerate that suite or its harness without
an explicit human request specifically authorizing end-to-end test work.
