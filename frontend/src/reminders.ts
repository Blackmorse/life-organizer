import type { ReminderTask } from './types'

export function reminderDate(task: ReminderTask) {
  return `${new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short', timeZone: task.timezone }).format(task.due_at)} (${task.timezone})`
}

export function normalizeReminders(items: ReminderTask[]) {
  return items.map((item) => item.expires_at !== null && item.expires_at <= Date.now() ? { ...item, expires_at: null } : item)
}
