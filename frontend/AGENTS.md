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
- Bot messaging, public hosting, and deployment are not implemented.

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
| `src/components/TaskForm.tsx` | Shared create/edit form with category selection. |
| `src/components/TaskCard.tsx` | Compact task row, expandable read view, and edit/move/delete actions. |
| `src/components/Subtasks.tsx` | Numbered checklist, progress, direct add, and per-row inline edit/delete/reorder actions. |
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
  flags. Steps have no independent IDs, archive, or nested children; parent task
  deletion removes its steps. List writes remain atomic and last-write-wins.
  Checkbox/delete/reorder failures keep the server-confirmed list; failed inline
  add/edit saves retain typed descriptions. Description edits preserve completion.
- Task responses include urgency (`someday`, `upcoming`, or `soon`). Always send
  urgency on writes, including task moves; omitted API input defaults to Someday.
  Extend the catalog in `src/urgency.ts` alongside the backend enum and a new
  database constraint migration. These horizons do not create due dates.
- IDs are generated by the backend; never use descriptions as keys.
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
