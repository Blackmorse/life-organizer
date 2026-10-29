import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { AppTheme } from './theme'
import type { Category, Subtask, Task } from './types'

// Component-level tests run the real client and hook against a fake HTTP boundary.
function mockApi(initialCategories: Category[] = [], initialTasks: Task[] = []) {
  let categories = [...initialCategories]
  let tasks = [...initialTasks]
  let nextId = 100
  const fetchMock = vi.fn(async (url: string, options?: RequestInit) => {
    expect(new Headers(options?.headers).get('Authorization')).toBe('tma signed-test-init-data')
    const method = options?.method ?? 'GET'
    const [, , resource, rawId, child] = url.split('/')
    const id = Number(rawId)
    const input = options?.body ? JSON.parse(String(options.body)) : undefined
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
