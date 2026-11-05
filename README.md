# Life Organizer

Life Organizer is a personal task organizer built around Telegram. It helps you
keep track of what needs doing, choose what to focus on each day, and follow
through without losing sight of longer-term work.

## What it does

- **Organize your work:** Group tasks into categories, set their urgency, and
  break them into smaller steps.
- **Keep extra context:** Add optional detailed notes to tasks and steps, visible
  when you expand their Mini App details.
- **Plan your day:** Pick tasks or individual steps for today, plan ahead for
  tomorrow, and decide whether to carry unfinished work forward.
- **Keep routines and dates in view:** Set recurring tasks and one-time reminders
  alongside your everyday work.
- **Track progress:** Complete tasks and steps, with finished tasks kept in an
  archive.
- **Stay connected through Telegram:** Receive daily summaries and morning
  planning prompts, check your list, and mark daily work complete by voice.

## How you use it

Open the Telegram Mini App to browse and manage your tasks, routines, reminders,
and daily plan. The companion bot helps you review your day and update progress
from the same chat app.

Your daily plan stays linked to your original tasks: completing work updates
your progress, while removing it from the plan leaves the task available for
later.

## Development

The app uses a React frontend and a Rust backend with PostgreSQL.
See the [project guide](AGENTS.md) for architecture and container setup,
the [backend guide](backend/AGENTS.md) for backend development, and
the [frontend README](frontend/README.md) for local UI setup.
