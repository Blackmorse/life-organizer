import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { AppTheme } from './theme'
import type { Category, Task } from './types'

// Component-level tests run the real client and hook against a fake HTTP boundary.
function mockApi(initialCategories: Category[] = [], initialTasks: Task[] = []) {
  let categories = [...initialCategories]
  let tasks = [...initialTasks]
  let nextId = 100
  const fetchMock = vi.fn(async (url: string, options?: RequestInit) => {
    expect(new Headers(options?.headers).get('Authorization')).toBe('tma signed-test-init-data')
    const method = options?.method ?? 'GET'
    const [, , resource, rawId] = url.split('/')
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
        const task = { id: nextId++, ...input }
        tasks.push(task)
        return Response.json(task, { status: 201 })
      }
      if (method === 'PUT') {
        const task = { id, ...input }
        tasks = tasks.map((item) => item.id === id ? task : item)
        return Response.json(task)
      }
      if (method === 'DELETE') {
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
      { id: 1, category_id: 1, description: 'Build mini app', urgency: 'upcoming' },
      { id: 2, category_id: 2, description: 'Clean desk', urgency: 'someday' },
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
      { id: 1, category_id: 1, description: 'Build mini app', urgency: 'soon' },
      { id: 2, category_id: 2, description: 'Clean desk', urgency: 'someday' },
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
    mockApi([{ id: 1, user_id: 1, description: 'Work' }, { id: 2, user_id: 1, description: 'Home' }], [{ id: 1, category_id: 1, description: 'First task', urgency: 'upcoming' }])
    const user = userEvent.setup()
    renderWorkspace()
    await screen.findByRole('button', { name: 'View task First task' })
    await categoryAction(user, 'Work', 'Delete category')
    expect(await screen.findByRole('alert')).toHaveTextContent('Move or delete its tasks first')
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
