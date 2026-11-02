import type { Category, DailyTask, Subtask, Task, TaskInput, ScheduledTask, ScheduledTaskInput, ScheduledOccurrence, ReminderTask, ReminderTaskInput } from './types'
import { getTelegramInitData } from './telegram'

export class AuthorizationError extends Error {}

async function request<T>(path: string, method = 'GET', body?: unknown, signal?: AbortSignal): Promise<T> {
  const initData = getTelegramInitData()
  if (!initData) throw new AuthorizationError('Open Scheduler from your bot’s menu button inside Telegram to access your personal workspace.')
  const headers: Record<string, string> = { Authorization: `tma ${initData}` }
  if (body !== undefined) headers['Content-Type'] = 'application/json'
  let response: Response
  try {
    response = await fetch(`/api${path}`, {
      method,
      signal,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  } catch (error) {
    if (signal?.aborted) throw error
    throw new Error('Cannot reach the API. Check that the frontend and Rust backend are running.')
  }

  if (!response.ok) {
    if (response.status === 401) throw new AuthorizationError('Telegram authorization expired or was rejected. Close and reopen the Mini App from your bot’s menu button.')
    const text = await response.text()
    let message = text || `API request failed (${response.status}).`
    try {
      const payload: unknown = JSON.parse(text)
      if (typeof payload === 'object' && payload !== null && 'error' in payload && typeof payload.error === 'string') {
        message = payload.error
      }
    } catch {
      // Axum extractor errors and development proxy failures can be plain text.
    }
    if (response.status === 409 && path.startsWith('/categories/')) message += '. Move or delete its active tasks first; restore archived tasks before moving or deleting them.'
    if (response.status >= 500 && !(path === '/daily-tasks/finalize' && response.status === 502)) message = 'The API is unavailable. Check the Rust server and database, then retry.'
    throw new Error(message)
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export const api = {
  listReminders: (signal?: AbortSignal) => request<ReminderTask[]>('/reminder-tasks', 'GET', undefined, signal),
  saveReminder: (input: ReminderTaskInput, id?: number) => request<ReminderTask>(id === undefined ? '/reminder-tasks' : `/reminder-tasks/${id}`, id === undefined ? 'POST' : 'PUT', input),
  deleteReminder: (id: number) => request<void>(`/reminder-tasks/${id}`, 'DELETE'),
  completeReminder: (id: number, completed: boolean) => request<ReminderTask>(`/reminder-tasks/${id}/completion`, 'PUT', { completed }),
  dismissReminder: (id: number) => request<void>(`/reminder-tasks/${id}/daily`, 'DELETE'),
  listScheduledTasks: (signal?: AbortSignal) => request<ScheduledTask[]>('/scheduled-tasks', 'GET', undefined, signal),
  saveScheduledTask: (input: ScheduledTaskInput, id?: number) => request<ScheduledTask>(id === undefined ? '/scheduled-tasks' : `/scheduled-tasks/${id}`, id === undefined ? 'POST' : 'PUT', input),
  deleteScheduledTask: (id: number) => request<void>(`/scheduled-tasks/${id}`, 'DELETE'),
  listScheduledDaily: (signal?: AbortSignal) => request<ScheduledOccurrence[]>('/scheduled-tasks/daily', 'GET', undefined, signal),
  completeScheduledDaily: (id: number, completed: boolean) => request<ScheduledOccurrence[]>(`/scheduled-tasks/daily/${id}/completion`, 'PUT', { completed }),
  replaceScheduledDailySubtasks: (id: number, subtasks: Subtask[]) => request<ScheduledOccurrence[]>(`/scheduled-tasks/daily/${id}/subtasks`, 'PUT', { subtasks }),
  dismissScheduledDaily: (id: number) => request<ScheduledOccurrence[]>(`/scheduled-tasks/daily/${id}`, 'DELETE'),
  finalizeDailyTasks: () => request<void>('/daily-tasks/finalize', 'POST'),
  listDailyTasks: (signal?: AbortSignal) => request<DailyTask[]>('/daily-tasks', 'GET', undefined, signal),
  selectDailyTask: (task_id: number, subtask_index?: number) => request<DailyTask[]>('/daily-tasks', 'POST', {
    task_id, subtask_index, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  }),
  removeDailyTask: (task_id: number, subtask_id: string) => request<DailyTask[]>('/daily-tasks', 'DELETE', { task_id, subtask_id }),
  listTomorrowTasks: (signal?: AbortSignal) => request<DailyTask[]>('/tomorrow-tasks', 'GET', undefined, signal),
  selectTomorrowTask: (task_id: number, subtask_index?: number) => request<DailyTask[]>('/tomorrow-tasks', 'POST', {
    task_id, subtask_index, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  }),
  removeTomorrowTask: (task_id: number, subtask_id: string) => request<DailyTask[]>('/tomorrow-tasks', 'DELETE', { task_id, subtask_id }),
  listCategories: (signal?: AbortSignal) => request<Category[]>('/categories', 'GET', undefined, signal),
  createCategory: (description: string) => request<Category>('/categories', 'POST', { description }),
  updateCategory: (id: number, description: string) => request<Category>(`/categories/${id}`, 'PUT', { description }),
  deleteCategory: (id: number) => request<void>(`/categories/${id}`, 'DELETE'),
  listTasks: (signal?: AbortSignal) => request<Task[]>('/tasks', 'GET', undefined, signal),
  createTask: (input: TaskInput) => request<Task>('/tasks', 'POST', input),
  updateTask: (id: number, input: TaskInput) => request<Task>(`/tasks/${id}`, 'PUT', input),
  replaceSubtasks: (id: number, subtasks: Subtask[]) => request<Task>(`/tasks/${id}/subtasks`, 'PUT', { subtasks }),
  setCompleted: (id: number, completed: boolean) => request<Task>(`/tasks/${id}/completion`, 'PUT', { completed }),
  deleteTask: (id: number) => request<void>(`/tasks/${id}`, 'DELETE'),
}
