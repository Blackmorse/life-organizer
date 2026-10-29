# Scheduler frontend

React + TypeScript + Vite + Material UI app for the Rust backend in `../backend`.

## Features

- Browse an expandable category/task hierarchy, with per-category counts.
- Open task rows to read full details; collapse branches or expand/collapse all.
- Search category names and task descriptions; matching branches expand automatically.
- Create, rename, and delete categories through on-demand forms and action menus.
- Create, edit, move, and delete tasks without keeping creation forms on screen.
- Choose task urgency: **Someday** (at some point in the future), **Upcoming**
  (next few weeks), or **Soon** (next few days). Labels appear on task rows;
  create/edit forms let you change them.
- Loading/saving indicators, refresh, empty states, and API errors.
- Failed writes preserve form contents for retry.
- Material UI components, responsive layout, light/dark themes, and an optional
  Telegram SDK bridge with live theme updates.
- Personal workspaces using verified Telegram Mini App authorization. Each user
  sees only their own categories and tasks.

Tasks have descriptions, categories, and urgency. New and existing tasks default
to Someday; restarting the backend applies the additive urgency migration
automatically. These are planning horizons, not enforced due dates.
Completion status, subcategories, and
implementation plans are not part of the current backend contract.

## Local development

Use Node 24.15.0 or newer within Node 24 (see `.nvmrc` and `package.json`).
With nvm installed, run `nvm install` and `nvm use` from this directory.

Start the Rust backend in a separate terminal, from `backend/`, using an existing
PostgreSQL database:

```sh
DATABASE_URL=postgres://user:password@localhost:5432/scheduler \
TELEGRAM_BOT_TOKEN='<your BotFather token>' cargo run
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
