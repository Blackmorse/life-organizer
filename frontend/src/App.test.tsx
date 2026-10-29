import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { AppTheme } from './theme'
import type { Category, DailyTask, Subtask, Task, ScheduledTask, ScheduledOccurrence } from './types'

// Component-level tests run the real client and hook against a fake HTTP boundary.
function mockApi(initialCategories: Category[] = [], initialTasks: Task[] = []) {
  let categories = [...initialCategories]
  let tasks = [...initialTasks]
  let daily: DailyTask[] = []
  let scheduled: ScheduledTask[] = []
  let occurrences: ScheduledOccurrence[] = []
  const skipped = new Set<number>()
  let nextId = 100
  const fetchMock = vi.fn(async (url: string, options?: RequestInit) => {
    expect(new Headers(options?.headers).get('Authorization')).toBe('tma signed-test-init-data')
    const method = options?.method ?? 'GET'
    const [, , resource, rawId, child] = url.split('/')
    const id = Number(rawId)
    const input = options?.body ? JSON.parse(String(options.body)) : undefined
    if (resource === 'scheduled-tasks') {
      if (rawId === 'daily') {
        if (method === 'GET') {
          for (const task of scheduled) {
            if (!skipped.has(task.id) && !occurrences.some((item) => item.scheduled_task_id === task.id)) occurrences.push({
              id: nextId++, scheduled_task_id: task.id, description: task.description,
              subtasks: task.subtasks.map((step) => ({ ...step, completed: false })), completed: false, expires_at: Date.now() + 86_400_000,
            })
          }
        }
        const occurrenceId = Number(child)
        if (method === 'PUT') occurrences = occurrences.map((item) => item.id !== occurrenceId ? item : input.subtasks
          ? { ...item, subtasks: input.subtasks, completed: input.subtasks.length > 0 && input.subtasks.every((step: Subtask) => step.completed) }
          : { ...item, completed: input.completed })
        if (method === 'DELETE') {
          const item = occurrences.find((item) => item.id === occurrenceId)!
          skipped.add(item.scheduled_task_id)
          occurrences = occurrences.filter((item) => item.id !== occurrenceId)
        }
        return Response.json(occurrences)
      }
      if (method === 'GET') return Response.json(scheduled)
      if (method === 'POST') {
        const task = { ...input, id: nextId++ }
        scheduled.push(task)
        return Response.json(task, { status: 201 })
      }
      if (method === 'PUT') {
        const task = { ...input, id }
        scheduled = scheduled.map((item) => item.id === id ? task : item)
        return Response.json(task)
      }
      if (method === 'DELETE') {
        scheduled = scheduled.filter((item) => item.id !== id)
        occurrences = occurrences.filter((item) => item.scheduled_task_id !== id)
        return new Response(null, { status: 204 })
      }
    }
    if (resource === 'daily-tasks') {
      if (rawId === 'finalize' && method === 'POST') return new Response(null, { status: 204 })
      daily = daily.filter((item) => item.expires_at > Date.now())
      if (method === 'POST') {
        const task = tasks.find((item) => item.id === input.task_id)!
        const step = input.subtask_index === undefined ? undefined : task.subtasks[input.subtask_index]
        if (step && !step.id) step.id = String(nextId++)
        const subtask_id = step?.id ?? ''
        if (!daily.some((item) => item.task_id === task.id && item.subtask_id === subtask_id)) {
          daily.push({ task_id: task.id, subtask_id, expires_at: Date.now() + 86_400_000 })
        }
      }
      if (method === 'DELETE') daily = daily.filter((item) => item.task_id !== input.task_id || item.subtask_id !== input.subtask_id)
      return Response.json(daily)
    }
    if (resource === 'categories') {
      if (method === 'GET') return Response.json(categories)
      if (method === 'POST') {
        const category = { id: nextId++, user_id: 1, description: input.description }
        categories.push(category)
        return Response.json(category, { status: 201 })
      }
      if (method === 'PUT') {
        const category = { id, user_id: 1, description: input.description }
        categories = categories.map((item) => item.id === id ? category : item)
        return Response.json(category)
      }
      if (method === 'DELETE') {
        if (tasks.some((task) => task.category_id === id)) return Response.json({ error: 'category still contains tasks' }, { status: 409 })
        categories = categories.filter((item) => item.id !== id)
        return new Response(null, { status: 204 })
      }
    }
    if (resource === 'tasks') {
      if (method === 'GET') return Response.json(tasks)
      if (method === 'POST') {
        const task = { id: nextId++, subtasks: [], completed: false, ...input }
        tasks.push(task)
        return Response.json(task, { status: 201 })
      }
      if (method === 'PUT') {
        const existing = tasks.find((item) => item.id === id)!
        if (existing.completed && child !== 'completion') return Response.json({ error: 'archived tasks are read-only; restore the task first' }, { status: 409 })
        const task = child === 'subtasks' ? {
          ...existing, subtasks: input.subtasks,
          completed: input.subtasks.length > 0 && input.subtasks.every((step: Subtask) => step.completed),
        } : { ...existing, ...input }
        tasks = tasks.map((item) => item.id === id ? task : item)
        return Response.json(task)
      }
      if (method === 'DELETE') {
        if (tasks.find((item) => item.id === id)?.completed) return Response.json({ error: 'archived tasks are read-only; restore the task first' }, { status: 409 })
        tasks = tasks.filter((item) => item.id !== id)
        return new Response(null, { status: 204 })
      }
    }
    throw new Error(`Unexpected request: ${method} ${url}`)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

describe('workspace', () => {
  beforeEach(() => {
    vi.stubGlobal('Telegram', { WebApp: { initData: 'signed-test-init-data', ready: vi.fn(), expand: vi.fn() } })
  })

  function renderWorkspace() {
    return render(<AppTheme><App /></AppTheme>)
  }

  async function categoryAction(user: ReturnType<typeof userEvent.setup>, category: string, action: string) {
    await user.click(screen.getByRole('button', { name: `Actions for category ${category}` }))
    await user.click(await screen.findByRole('menuitem', { name: action }))
  }

  it('creates required weekday schedules, edits future details, and tracks/skips today independently', async () => {
    const fetchMock = mockApi()
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByText('Your workspace is empty')
    await user.click(screen.getByRole('tab', { name: 'Scheduled tasks' }))
    await user.click(screen.getByRole('button', { name: 'Add scheduled task' }))
    await user.type(screen.getByRole('textbox', { name: 'Scheduled task description' }), 'Sport exercises')
    expect(screen.queryByRole('combobox', { name: 'Urgency' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Save schedule' })).toBeDisabled()
    for (const day of ['Monday', 'Wednesday', 'Friday']) await user.click(screen.getByRole('checkbox', { name: day }))
    await user.click(screen.getByRole('button', { name: 'Add subtask' }))
    expect(screen.getByRole('button', { name: 'Save schedule' })).toBeDisabled()
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), 'Warm up')
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    await user.click(screen.getByRole('button', { name: 'Save schedule' }))
    await screen.findByRole('button', { name: 'View scheduled task Sport exercises' })
    const create = fetchMock.mock.calls.find(([url, options]) => url === '/api/scheduled-tasks' && options?.method === 'POST')!
    const payload = JSON.parse(String(create[1]?.body))
    expect(payload.weekdays).toEqual([1, 3, 5])
    expect(payload).not.toHaveProperty('category_id')
    expect(payload).not.toHaveProperty('urgency')
    expect(payload.subtasks).toEqual([{ description: 'Warm up', completed: false }])
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    await user.click(screen.getByRole('button', { name: 'View daily scheduled task Sport exercises' }))
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('checkbox', { name: 'Subtask 1: Warm up' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'Subtask 1: Warm up' })).not.toBeChecked()
    expect(fetchMock.mock.calls.some(([url]) => url === '/api/daily-tasks/finalize')).toBe(false)
    await user.click(screen.getByRole('checkbox', { name: 'Subtask 1: Warm up' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Sport exercises' })).toBeChecked()
    expect(fetchMock.mock.calls.some(([url]) => url === '/api/daily-tasks/finalize')).toBe(true)
    await user.click(screen.getByRole('tab', { name: 'Scheduled tasks' }))
    await user.click(screen.getByRole('button', { name: 'View scheduled task Sport exercises' }))
    await user.click(screen.getByRole('button', { name: 'Edit schedule' }))
    const description = screen.getByRole('textbox', { name: 'Scheduled task description' })
    await user.clear(description)
    await user.type(description, 'New exercises')
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Save schedule' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(description).toHaveValue('New exercises')
    await user.click(screen.getByRole('button', { name: 'Save schedule' }))
    await screen.findByRole('button', { name: 'View scheduled task New exercises' })
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Sport exercises' })).toBeChecked()
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Remove from Tasks of the day: Sport exercises' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'Complete daily Sport exercises' })).toBeChecked()
    await user.click(screen.getByRole('button', { name: 'Remove from Tasks of the day: Sport exercises' }))
    expect(screen.queryByRole('checkbox', { name: 'Complete daily Sport exercises' })).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    expect(screen.queryByRole('checkbox', { name: 'Complete daily Sport exercises' })).not.toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: 'Scheduled tasks' }))
    await user.click(screen.getByRole('button', { name: 'View scheduled task New exercises' }))
    await user.click(screen.getByRole('button', { name: 'Delete scheduled task' }))
    expect(await screen.findByText('No scheduled tasks yet.')).toBeInTheDocument()
  })

  it('finalizes all daily picks regardless of search, disables duplicate sends, and keeps list editable', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build', urgency: 'soon', completed: false, subtasks: [] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByRole('button', { name: 'View task Build' })
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    expect(screen.getByRole('button', { name: 'Finalize tasks' })).toBeDisabled()
    await user.click(screen.getByRole('tab', { name: 'Active' }))
    await user.click(screen.getByRole('button', { name: 'Add to Tasks of the day: Build' }))
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    await user.type(screen.getByRole('searchbox'), 'no match')
    const button = screen.getByRole('button', { name: 'Finalize tasks' })
    expect(button).toBeEnabled()
    let finish!: (response: Response) => void
    fetchMock.mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve }))
    await user.dblClick(button)
    expect(button).toBeDisabled()
    expect(fetchMock.mock.calls.filter(([url]) => url === '/api/daily-tasks/finalize')).toHaveLength(1)
    await act(async () => { finish(new Response(null, { status: 204 })) })
    expect(await screen.findByText("Today's tasks sent to your Telegram bot chat.")).toBeInTheDocument()
    expect(button).toBeEnabled()
    expect(fetchMock).toHaveBeenCalledWith('/api/daily-tasks/finalize', expect.objectContaining({ method: 'POST', body: undefined }))
    await user.click(screen.getByRole('button', { name: 'Clear search' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).not.toBeChecked()
    await user.click(screen.getByRole('button', { name: 'Remove from Tasks of the day: Build' }))
    expect(button).toBeDisabled()
    expect(screen.queryByText("Today's tasks sent to your Telegram bot chat.")).not.toBeInTheDocument()
  })

  it('clears expired scheduled occurrences on resume even while refresh is stalled', async () => {
    const fetchMock = mockApi()
    const handleRequest = fetchMock.getMockImplementation()!
    const occurrence: ScheduledOccurrence = {
      id: 1, scheduled_task_id: 1, description: 'Daily exercise',
      subtasks: [], completed: false, expires_at: Date.now() + 60_000,
    }
    fetchMock.mockImplementation((url, options) => url === '/api/scheduled-tasks/daily'
      ? Promise.resolve(Response.json([occurrence])) : handleRequest(url, options))
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByText('Your workspace is empty')
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Daily exercise' })).toBeInTheDocument()
    fetchMock.mockImplementationOnce(() => new Promise<Response>(() => {}))
    const clock = vi.spyOn(Date, 'now').mockReturnValue(occurrence.expires_at + 1)
    try {
      fireEvent.focus(window)
      expect(screen.queryByRole('checkbox', { name: 'Complete daily Daily exercise' })).not.toBeInTheDocument()
      expect(screen.getByRole('button', { name: 'Finalize tasks' })).toBeDisabled()
    } finally { clock.mockRestore() }
  })

  it('shows bot start and delivery guidance on finalize failures, preserving picks for retry', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build', urgency: 'soon', completed: false, subtasks: [] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'Add to Tasks of the day: Build' }))
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    for (const [status, error] of [
      [400, "Open the bot's private chat and send /start."],
      [502, 'Could not confirm Telegram delivery. Check the bot chat before retrying; some messages may already have arrived.'],
    ] as const) {
      fetchMock.mockResolvedValueOnce(Response.json({ error }, { status }))
      await user.click(screen.getByRole('button', { name: 'Finalize tasks' }))
      expect(await screen.findByRole('alert')).toHaveTextContent(error)
      expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).toBeInTheDocument()
      expect(screen.queryByText("Today's tasks sent to your Telegram bot chat.")).not.toBeInTheDocument()
    }
    await user.click(screen.getByRole('button', { name: 'Finalize tasks' }))
    expect(await screen.findByText("Today's tasks sent to your Telegram bot chat.")).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('tracks task and subtask picks, keeps reordered edits linked, and completes original work', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build', urgency: 'soon', completed: false, subtasks: [
        { description: 'Design', completed: false }, { description: 'Ship', completed: false },
      ] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'Add to Tasks of the day: Build' }))
    await user.click(screen.getByRole('button', { name: 'View task Build' }))
    await user.click(screen.getByRole('button', { name: 'Add to Tasks of the day: Ship' }))
    await user.click(screen.getByRole('button', { name: 'Move subtask 2 up' }))
    await user.click(screen.getByRole('button', { name: 'Edit subtask 1' }))
    await user.clear(screen.getByRole('textbox', { name: 'Subtask description' }))
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), 'Release')
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).not.toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'Complete daily Release' })).not.toBeChecked()
    expect(screen.queryByRole('checkbox', { name: 'Complete daily Design' })).not.toBeInTheDocument()
    const sends = () => fetchMock.mock.calls.filter(([url]) => url === '/api/daily-tasks/finalize')
    expect(sends()).toHaveLength(0)
    await user.click(screen.getByRole('checkbox', { name: 'Complete daily Release' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Release' })).toBeChecked()
    expect(sends()).toHaveLength(1)
    expect(fetchMock.mock.calls.slice(-2).map(([url]) => url)).toEqual(['/api/tasks/1/subtasks', '/api/daily-tasks/finalize'])
    await user.click(screen.getByRole('checkbox', { name: 'Complete daily Release' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Release' })).not.toBeChecked()
    expect(sends()).toHaveLength(2)
    await user.click(screen.getByRole('checkbox', { name: 'Complete daily Release' }))
    expect(sends()).toHaveLength(3)
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    expect(screen.getByRole('checkbox', { name: 'Complete daily Release' })).toBeChecked()
    await user.click(screen.getByRole('checkbox', { name: 'Complete daily Build' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'Complete daily Release' })).toBeDisabled()
    expect(sends()).toHaveLength(4)
    expect(fetchMock.mock.calls.slice(-2).map(([url]) => url)).toEqual(['/api/tasks/1/completion', '/api/daily-tasks/finalize'])
    await user.click(screen.getByRole('button', { name: 'Remove from Tasks of the day: Release' }))
    expect(screen.queryByRole('checkbox', { name: 'Complete daily Release' })).not.toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: 'Archive' }))
    await user.click(screen.getByRole('button', { name: 'View task Build' }))
    expect(screen.getByRole('checkbox', { name: 'Subtask 1: Release' })).toBeChecked()
    expect(fetchMock).toHaveBeenCalledWith('/api/daily-tasks', expect.objectContaining({
      method: 'POST', body: JSON.stringify({ task_id: 1, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone }),
    }))
  })

  it('sends only after completion saves and retains saved progress when automatic delivery fails', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build', urgency: 'soon', completed: false, subtasks: [] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'Add to Tasks of the day: Build' }))
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    const checkbox = screen.getByRole('checkbox', { name: 'Complete daily Build' })
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(checkbox)
    expect(checkbox).not.toBeChecked()
    expect(fetchMock.mock.calls.filter(([url]) => url === '/api/daily-tasks/finalize')).toHaveLength(0)

    const handleRequest = fetchMock.getMockImplementation()!
    let finish!: (response: Response) => void
    fetchMock.mockImplementation(async (url, options) => url === '/api/daily-tasks/finalize'
      ? new Promise<Response>((resolve) => { finish = resolve })
      : handleRequest(url, options))
    await user.click(checkbox)
    expect(checkbox).toBeChecked()
    expect(checkbox).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Finalize tasks' })).toBeDisabled()
    await act(async () => { finish(Response.json({ error: 'Check the bot chat before retrying; some messages may already have arrived.' }, { status: 502 })) })
    expect(await screen.findByRole('alert')).toHaveTextContent('Progress saved, but the Telegram summary could not be sent.')
    expect(screen.getByRole('alert')).toHaveTextContent('Check the bot chat before retrying')
    expect(checkbox).toBeChecked()
    expect(checkbox).toBeEnabled()
    fetchMock.mockImplementation(handleRequest)
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).toBeChecked()
    await user.click(screen.getByRole('button', { name: 'Finalize tasks' }))
    expect(await screen.findByText("Today's tasks sent to your Telegram bot chat.")).toBeInTheDocument()
  })

  it('resends a picked parent when its last subtask completes from Active', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build', urgency: 'soon', completed: false, subtasks: [{ description: 'Ship', completed: false }] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'Add to Tasks of the day: Build' }))
    await user.click(screen.getByRole('button', { name: 'View task Build' }))
    await user.click(screen.getByRole('checkbox', { name: 'Subtask 1: Ship' }))
    expect(fetchMock.mock.calls.filter(([url]) => url === '/api/daily-tasks/finalize')).toHaveLength(1)
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).toBeChecked()
  })

  it('keeps daily picks on failed removal and clears expired picks when app resumes', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build', urgency: 'soon', completed: false, subtasks: [] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'Add to Tasks of the day: Build' }))
    await user.click(screen.getByRole('tab', { name: 'Tasks of the day' }))
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Remove from Tasks of the day: Build' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The API is unavailable')
    expect(screen.getByRole('checkbox', { name: 'Complete daily Build' })).toBeInTheDocument()
    const clock = vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 86_400_001)
    fireEvent.focus(window)
    expect(screen.queryByRole('checkbox', { name: 'Complete daily Build' })).not.toBeInTheDocument()
    clock.mockRestore()
    await user.click(screen.getByRole('tab', { name: 'Active' }))
    expect(screen.getByRole('button', { name: 'View task Build' })).toBeInTheDocument()
  })

  it('starts with a category/task hierarchy and opens task details without showing creation forms', async () => {
    mockApi([{ id: 1, user_id: 1, description: 'Work' }, { id: 2, user_id: 1, description: 'Home' }], [
      { id: 1, category_id: 1, description: 'Build mini app', urgency: 'upcoming', subtasks: [], completed: false },
      { id: 2, category_id: 2, description: 'Clean desk', urgency: 'someday', subtasks: [], completed: false },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    const task = await screen.findByRole('button', { name: 'View task Build mini app' })
    expect(within(task).getByText('Upcoming')).toBeInTheDocument()
    expect(within(screen.getByRole('list', { name: 'Tasks in Work' })).getByRole('button', { name: 'View task Build mini app' })).toBe(task)
    expect(within(screen.getByRole('list', { name: 'Tasks in Home' })).queryByRole('button', { name: 'View task Build mini app' })).not.toBeInTheDocument()
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit task Build mini app' })).not.toBeInTheDocument()
    await user.click(task)
    expect(task).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('button', { name: 'Edit task Build mini app' })).toBeInTheDocument()
    await user.click(task)
    expect(screen.queryByRole('button', { name: 'Edit task Build mini app' })).not.toBeInTheDocument()
  })

  it('collapses categories and searches across tasks, revealing matching branches', async () => {
    mockApi([{ id: 1, user_id: 1, description: 'Work' }, { id: 2, user_id: 1, description: 'Home' }], [
      { id: 1, category_id: 1, description: 'Build mini app', urgency: 'soon', subtasks: [], completed: false },
      { id: 2, category_id: 2, description: 'Clean desk', urgency: 'someday', subtasks: [], completed: false },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByRole('button', { name: 'View task Build mini app' })
    await user.click(screen.getByRole('button', { name: 'Collapse all' }))
    expect(screen.queryByRole('button', { name: 'View task Build mini app' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Work, 1 task' })).toHaveAttribute('aria-expanded', 'false')
    await user.click(screen.getByRole('button', { name: 'Expand all' }))
    expect(screen.getByRole('button', { name: 'View task Build mini app' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Work, 1 task' }))
    await user.type(screen.getByRole('searchbox', { name: 'Search categories and tasks' }), 'MINI')
    expect(screen.getByRole('button', { name: 'View task Build mini app' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Home, 1 task' })).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Clear search' }))
    expect(screen.getByRole('button', { name: 'Work, 1 task' })).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByRole('button', { name: 'View task Clean desk' })).toBeInTheDocument()
    await user.type(screen.getByRole('searchbox'), 'no matching task')
    expect(screen.getByRole('heading', { name: 'No matches' })).toBeInTheDocument()
  })

  it('creates and renames a category, creates a task, and persists both across refresh', async () => {
    mockApi()
    const user = userEvent.setup()
    renderWorkspace()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Category' })).toBeEnabled())
    await user.click(screen.getByRole('button', { name: 'Category' }))
    await user.type(screen.getByRole('textbox', { name: /Category description/ }), 'Work')
    await user.click(screen.getByRole('button', { name: 'Create category' }))
    await screen.findByRole('button', { name: 'Work, 0 tasks' })
    await categoryAction(user, 'Work', 'Rename category')
    const categoryInput = screen.getByRole('textbox', { name: /Category description/ })
    await user.clear(categoryInput)
    await user.type(categoryInput, 'Projects')
    await user.click(screen.getByRole('button', { name: 'Save category' }))
    await screen.findByRole('button', { name: 'Projects, 0 tasks' })
    await user.click(screen.getByRole('button', { name: 'Add task to Projects' }))
    await user.type(screen.getByRole('textbox', { name: /Task description/ }), 'Build mini app')
    expect(screen.getByRole('combobox', { name: /Urgency/ })).toHaveValue('someday')
    await user.selectOptions(screen.getByRole('combobox', { name: /Urgency/ }), 'upcoming')
    await user.click(screen.getByRole('button', { name: 'Create task' }))
    await screen.findByRole('button', { name: 'View task Build mini app' })
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    expect(screen.getByRole('button', { name: 'View task Build mini app' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Projects, 1 task' })).toBeInTheDocument()
    expect(within(screen.getByRole('button', { name: 'View task Build mini app' })).getByText('Upcoming')).toBeInTheDocument()
  })

  it('reports category conflicts, moves tasks between branches, and deletes entities', async () => {
    mockApi([{ id: 1, user_id: 1, description: 'Work' }, { id: 2, user_id: 1, description: 'Home' }], [{ id: 1, category_id: 1, description: 'First task', urgency: 'upcoming', subtasks: [], completed: false }])
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByRole('button', { name: 'View task First task' })
    await categoryAction(user, 'Work', 'Delete category')
    expect(await screen.findByRole('alert')).toHaveTextContent('restore archived tasks before moving or deleting them')
    await user.click(screen.getByRole('button', { name: 'View task First task' }))
    await user.click(screen.getByRole('button', { name: 'Edit task First task' }))
    const editor = screen.getByRole('button', { name: 'Save changes' }).closest('form')!
    expect(within(editor).getByRole('combobox', { name: /Urgency/ })).toHaveValue('upcoming')
    await user.selectOptions(within(editor).getByRole('combobox', { name: /Urgency/ }), 'soon')
    await user.selectOptions(within(editor).getByRole('combobox', { name: /Category/ }), '2')
    await user.clear(within(editor).getByRole('textbox', { name: /Task description/ }))
    await user.type(within(editor).getByRole('textbox', { name: /Task description/ }), 'Moved task')
    await user.click(within(editor).getByRole('button', { name: 'Save changes' }))
    await screen.findByRole('button', { name: 'View task Moved task' })
    expect(within(screen.getByRole('button', { name: 'View task Moved task' })).getByText('Soon')).toBeInTheDocument()
    expect(within(screen.getByRole('list', { name: 'Tasks in Work' })).queryByRole('button', { name: 'View task Moved task' })).not.toBeInTheDocument()
    expect(within(screen.getByRole('list', { name: 'Tasks in Home' })).getByRole('button', { name: 'View task Moved task' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'View task Moved task' }))
    await user.click(screen.getByRole('button', { name: 'Delete task Moved task' }))
    await waitFor(() => expect(screen.queryByRole('button', { name: 'View task Moved task' })).not.toBeInTheDocument())
    await categoryAction(user, 'Home', 'Delete category')
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Home, 0 tasks' })).not.toBeInTheDocument())
    expect(screen.getByRole('button', { name: 'Work, 0 tasks' })).toBeInTheDocument()
  })

  it('recovers from an initial connection failure without reloading the page', async () => {
    const fetchMock = mockApi()
    fetchMock.mockRejectedValueOnce(new TypeError('Failed to fetch'))
    renderWorkspace()
    expect(await screen.findByRole('alert')).toHaveTextContent('Cannot reach the API')
    await userEvent.click(screen.getByRole('button', { name: 'Retry loading data' }))
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
    await waitFor(() => expect(screen.getByRole('button', { name: 'Category' })).toBeEnabled())
  })

  it('keeps an unsaved task draft after a failed write and allows retry', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'Add task to Work' }))
    await user.type(screen.getByRole('textbox', { name: /Task description/ }), 'Keep my draft')
    await user.selectOptions(screen.getByRole('combobox', { name: /Urgency/ }), 'soon')
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Create task' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Check the Rust server and database')
    expect(screen.getByRole('textbox', { name: /Task description/ })).toHaveValue('Keep my draft')
    expect(screen.getByRole('combobox', { name: /Urgency/ })).toHaveValue('soon')
    await user.click(screen.getByRole('button', { name: 'Create task' }))
    await screen.findByRole('button', { name: 'View task Keep my draft' })
    expect(screen.queryByRole('textbox', { name: /Task description/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('requires Telegram launch instead of making anonymous API requests', async () => {
    vi.stubGlobal('Telegram', undefined)
    const fetchMock = mockApi()
    renderWorkspace()
    expect(await screen.findByRole('alert')).toHaveTextContent('Open Scheduler from your bot’s menu button inside Telegram')
    expect(fetchMock).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Category' })).not.toBeInTheDocument()
  })

  it('completes tasks into a read-only archive and restores them, retaining state after failed requests', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }, { id: 2, user_id: 1, description: 'Empty' }], [
      { id: 1, category_id: 1, description: 'Ship app', urgency: 'soon', subtasks: [{ description: 'Deploy', completed: true }, { description: 'Verify', completed: false }], completed: false },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByRole('button', { name: 'View task Ship app' })
    await user.click(screen.getByRole('tab', { name: 'Archive' }))
    expect(screen.getByRole('tabpanel', { name: 'Archive' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Your archive is empty' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'View task Ship app' })).not.toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: 'Active' }))
    await user.click(screen.getByRole('button', { name: 'View task Ship app' }))
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Complete task Ship app' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The API is unavailable')
    expect(screen.getByRole('button', { name: 'View task Ship app' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Complete task Ship app' }))
    await waitFor(() => expect(screen.queryByRole('button', { name: 'View task Ship app' })).not.toBeInTheDocument())
    expect(screen.getByRole('button', { name: 'Work, 0 tasks' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Active · 0 tasks')
    expect(fetchMock).toHaveBeenCalledWith('/api/tasks/1/completion', expect.objectContaining({ method: 'PUT', body: JSON.stringify({ completed: true }) }))
    await user.click(screen.getByRole('tab', { name: 'Archive' }))
    expect(screen.getByRole('tab', { name: 'Archive' })).toHaveAttribute('aria-selected', 'true')
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    await user.click(screen.getByRole('button', { name: 'View task Ship app' }))
    expect(screen.getByRole('status')).toHaveTextContent('Archived · 1 task')
    expect(screen.getByRole('button', { name: 'Work, 1 task' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Empty, 0 tasks' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Category' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add task to Work' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Actions for category Work' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit task Ship app' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Delete task Ship app' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Complete task Ship app' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add subtask' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit subtask 1' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Delete subtask 1' })).not.toBeInTheDocument()
    expect(within(screen.getByRole('region', { name: 'Subtasks' })).getAllByRole('listitem').map((item) => item.textContent)).toEqual(['Deploy', 'Verify'])
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Restore task Ship app' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The API is unavailable')
    expect(screen.getByRole('button', { name: 'Restore task Ship app' })).toBeEnabled()
    await user.click(screen.getByRole('button', { name: 'Restore task Ship app' }))
    expect(await screen.findByRole('heading', { name: 'Your archive is empty' })).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith('/api/tasks/1/completion', expect.objectContaining({ method: 'PUT', body: JSON.stringify({ completed: false }) }))
    await user.click(screen.getByRole('tab', { name: 'Active' }))
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    const restored = screen.getByRole('button', { name: 'View task Ship app' })
    expect(within(restored).getByText('Soon')).toBeInTheDocument()
    await user.click(restored)
    expect(screen.getByRole('button', { name: 'Edit task Ship app' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete task Ship app' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Add subtask' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Edit subtask 1' })).toBeInTheDocument()
    expect(within(screen.getByRole('region', { name: 'Subtasks' })).getAllByRole('listitem').map((item) => item.textContent)).toEqual(['Deploy', 'Verify'])
  })

  it('loads existing archived tasks separately and searches within the selected tab', async () => {
    mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Active task', urgency: 'someday', subtasks: [], completed: false },
      { id: 2, category_id: 1, description: 'Finished task', urgency: 'upcoming', subtasks: [{ description: 'Unique archived step', completed: true }], completed: true },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByRole('button', { name: 'View task Active task' })
    expect(screen.queryByRole('button', { name: 'View task Finished task' })).not.toBeInTheDocument()
    await user.type(screen.getByRole('searchbox'), 'Unique archived step')
    expect(screen.getByRole('heading', { name: 'No matches' })).toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: 'Archive' }))
    expect(screen.getByRole('searchbox')).toHaveValue('')
    expect(screen.getByRole('button', { name: 'View task Finished task' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'View task Active task' })).not.toBeInTheDocument()
    await user.type(screen.getByRole('searchbox'), 'Unique archived step')
    expect(screen.getByRole('button', { name: 'View task Finished task' })).toBeInTheDocument()
    await user.clear(screen.getByRole('searchbox'))
    await user.type(screen.getByRole('searchbox'), 'no matching archive')
    expect(screen.getByRole('heading', { name: 'No matches' })).toBeInTheDocument()
  })

  it('checks and reopens subtasks, preserves progress through reordering, and archives the finished parent', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build app', urgency: 'soon', completed: false, subtasks: [
        { description: 'Design', completed: false }, { description: 'Build', completed: false },
      ] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'View task Build app' }))
    const firstStep = () => screen.getByRole('checkbox', { name: 'Subtask 1: Design' })
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(firstStep())
    expect(await screen.findByRole('alert')).toHaveTextContent('The API is unavailable')
    expect(firstStep()).not.toBeChecked()
    await user.click(firstStep())
    await waitFor(() => expect(firstStep()).toBeChecked())
    expect(screen.getByRole('button', { name: 'View task Build app' })).toBeInTheDocument()
    expect(screen.getByText('1 of 2 completed')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    expect(firstStep()).toBeChecked()
    await user.click(firstStep())
    await waitFor(() => expect(firstStep()).not.toBeChecked())
    await user.click(firstStep())
    await waitFor(() => expect(firstStep()).toBeChecked())
    await user.click(screen.getByRole('button', { name: 'Move subtask 1 down' }))
    expect(await screen.findByRole('checkbox', { name: 'Subtask 2: Design' })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'Subtask 1: Build' })).not.toBeChecked()
    await user.click(screen.getByRole('button', { name: 'Edit subtask 2' }))
    const description = screen.getByRole('textbox', { name: 'Subtask description' })
    expect(description).toHaveValue('Design')
    expect(description).toHaveFocus()
    await user.clear(description)
    await user.type(description, 'Design revised')
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    expect(await screen.findByRole('checkbox', { name: 'Subtask 2: Design revised' })).toBeChecked()
    const lastStep = () => screen.getByRole('checkbox', { name: 'Subtask 1: Build' })
    // A failed final check must not move the task into the archive.
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(lastStep())
    expect(await screen.findByRole('alert')).toHaveTextContent('The API is unavailable')
    expect(lastStep()).not.toBeChecked()
    await user.click(lastStep())
    await waitFor(() => expect(screen.queryByRole('button', { name: 'View task Build app' })).not.toBeInTheDocument())
    expect(fetchMock).toHaveBeenCalledWith('/api/tasks/1/subtasks', expect.objectContaining({ method: 'PUT', body: JSON.stringify({ subtasks: [
      { description: 'Build', completed: true }, { description: 'Design revised', completed: true },
    ] }) }))
    await user.click(screen.getByRole('tab', { name: 'Archive' }))
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    await user.click(screen.getByRole('button', { name: 'View task Build app' }))
    const archivedSteps = within(screen.getByRole('region', { name: 'Subtasks' })).getAllByRole('checkbox')
    expect(archivedSteps).toHaveLength(2)
    for (const checkbox of archivedSteps) {
      expect(checkbox).toBeChecked()
      expect(checkbox).toBeDisabled()
    }
    await user.click(screen.getByRole('button', { name: 'Restore task Build app' }))
    await screen.findByRole('heading', { name: 'Your archive is empty' })
    await user.click(screen.getByRole('tab', { name: 'Active' }))
    await user.click(screen.getByRole('button', { name: 'View task Build app' }))
    expect(lastStep()).toBeChecked()
    expect(lastStep()).toBeEnabled()
    await user.click(lastStep())
    await waitFor(() => expect(lastStep()).not.toBeChecked())
    expect(screen.getByRole('checkbox', { name: 'Subtask 2: Design revised' })).toBeChecked()
  })

  it('deletes directly from a subtask row and archives when only completed steps remain', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'One step', urgency: 'someday', completed: false, subtasks: [
        { description: 'Finished', completed: true }, { description: 'Not needed', completed: false },
      ] },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'View task One step' }))
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Delete subtask 2' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The API is unavailable')
    expect(screen.getByRole('checkbox', { name: 'Subtask 1: Finished' })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'Subtask 2: Not needed' })).not.toBeChecked()
    await user.click(screen.getByRole('button', { name: 'Delete subtask 2' }))
    await waitFor(() => expect(screen.queryByRole('button', { name: 'View task One step' })).not.toBeInTheDocument())
    await user.click(screen.getByRole('tab', { name: 'Archive' }))
    expect(screen.getByRole('button', { name: 'View task One step' })).toBeInTheDocument()
  })

  it('adds and edits subtasks inline, reorders and deletes directly, and keeps failed drafts', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Work' }], [
      { id: 1, category_id: 1, description: 'Build app', urgency: 'soon', subtasks: [], completed: false },
    ])
    const user = userEvent.setup()
    renderWorkspace()
    await user.click(await screen.findByRole('button', { name: 'View task Build app' }))
    await user.click(screen.getByRole('button', { name: 'Add subtask' }))
    expect(screen.getByRole('textbox', { name: 'Subtask description' })).toHaveFocus()
    expect(screen.getByRole('button', { name: 'Save subtask' })).toBeDisabled()
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), '   ')
    expect(screen.getByRole('button', { name: 'Save subtask' })).toBeDisabled()
    await user.clear(screen.getByRole('textbox', { name: 'Subtask description' }))
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), 'Design')
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Subtask description' })).toHaveValue('Design')
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    await screen.findByRole('checkbox', { name: 'Subtask 1: Design' })
    await user.click(screen.getByRole('button', { name: 'Add subtask' }))
    expect(screen.getByRole('textbox', { name: 'Subtask description' })).toHaveFocus()
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), 'Implement')
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    await screen.findByRole('checkbox', { name: 'Subtask 2: Implement' })
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Move subtask 2 up' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'Subtask 1: Design' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Move subtask 2 up' }))
    await screen.findByRole('checkbox', { name: 'Subtask 1: Implement' })
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    const section = screen.getByRole('region', { name: 'Subtasks' })
    expect(within(section).getAllByRole('listitem').map((item) => item.textContent)).toEqual(['Implement', 'Design'])
    await user.type(screen.getByRole('searchbox'), 'Implement')
    expect(screen.getByRole('button', { name: 'View task Build app' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Clear search' }))
    await user.click(screen.getByRole('button', { name: 'Edit subtask 1' }))
    await user.clear(screen.getByRole('textbox', { name: 'Subtask description' }))
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), 'Build')
    expect(screen.getByRole('button', { name: 'Delete subtask 2' })).toBeDisabled()
    fetchMock.mockResolvedValueOnce(Response.json({ error: 'storage operation failed' }, { status: 500 }))
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    expect(await screen.findByRole('alert')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Subtask description' })).toHaveValue('Build')
    await user.click(screen.getByRole('button', { name: 'Save subtask' }))
    await screen.findByRole('checkbox', { name: 'Subtask 1: Build' })
    await user.click(screen.getByRole('button', { name: 'Edit subtask 1' }))
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), ' discarded')
    await user.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(within(section).getByText('Build')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Add subtask' }))
    await user.type(screen.getByRole('textbox', { name: 'Subtask description' }), 'Discarded new step')
    await user.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(within(section).queryByText('Discarded new step')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Delete subtask 2' }))
    await waitFor(() => expect(within(section).getAllByRole('listitem')).toHaveLength(1))
    await user.click(screen.getByRole('button', { name: 'Delete subtask 1' }))
    expect(await screen.findByText('No subtasks yet.')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeEnabled())
    expect(screen.getByText('No subtasks yet.')).toBeInTheDocument()
  })

  it('asks to reopen Telegram and hides private data when authorization expires', async () => {
    const fetchMock = mockApi([{ id: 1, user_id: 1, description: 'Private work' }])
    renderWorkspace()
    await screen.findByRole('button', { name: 'Private work, 0 tasks' })
    fetchMock.mockResolvedValue(Response.json({ error: 'invalid authorization' }, { status: 401 }))
    await userEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Close and reopen the Mini App')
    expect(screen.queryByRole('button', { name: 'Private work, 0 tasks' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Category' })).not.toBeInTheDocument()
  })
})
