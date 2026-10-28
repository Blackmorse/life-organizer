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
| `frontend/` (planned) | React | Telegram Mini App UI, with a possible standalone web UI using the same frontend. |

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

- **Tasks:** Personal work items to track and manage.
- **Categories and subcategories:** Hierarchical organization of tasks.
- **Implementation plans:** Plans describing how to carry out a task.
- **Notifications:** Telegram messages about relevant tasks or events; detailed notification behavior is not yet defined.

## Current repository stage

`backend/` is a Rust Axum/Tokio server with PostgreSQL storage through SQLx.
Category and task CRUD are available through DAO, service, and HTTP API layers.
Each task belongs to exactly one category. Versioned SQLx migrations run at
startup. Database-backed integration tests use a fresh Testcontainers PostgreSQL
container per test. Backend-specific architecture, API contract, testing rules,
and build/run/test commands live in `backend/AGENTS.md`.

`frontend/` has not been created. Do not scaffold it until requested.

Rust, React, PostgreSQL, Axum, Tokio, and SQLx are decided. Frontend tooling,
authentication, deployment, and schemas for future features remain undecided.
Telegram integration, subcategories, and implementation plans are not implemented yet.

## Guidance for future changes

- Preserve the two-subproject monorepo structure unless explicitly asked to change it.
- Keep the bot within the backend initially.
- Keep domain logic independent of UI and Telegram transport details.
- Split independent backend responsibilities into focused modules.
- Cover backend behavior with tests. End-to-end tests are primarily human-maintained;
  agents must not change that suite or its supporting artifacts without an explicit
  human request specifically authorizing end-to-end test work. See `backend/AGENTS.md`.
- Update this guide when architecture decisions or repository layout change.
- Add build, run, and test instructions once the relevant subprojects and tooling exist.
