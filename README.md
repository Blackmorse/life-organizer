# Life Organizer

Personal task organizer with a Rust API/Telegram bot and React Mini App.
See `backend/AGENTS.md` and `frontend/README.md` for local development.

## Container images

Separate GitHub Actions workflows publish images to this repository's GitHub
Packages (GitHub Container Registry) on pushes to `main`:

| Changed paths | Image |
| --- | --- |
| `backend/**` | `ghcr.io/blackmorse/life-organizer/backend` |
| `frontend/**` | `ghcr.io/blackmorse/life-organizer/frontend` |

Both images support `linux/amd64` and `linux/arm64`, with `latest` and
`sha-<full-commit-sha>` tags. Workflows authenticate with the built-in
`GITHUB_TOKEN` using `packages: write`; no registry secret is needed.
All build and runtime stages use Alpine-based images.

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
