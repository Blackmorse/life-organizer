import type { ReminderTask } from './types'

export function reminderDate(task: ReminderTask) {
  return `${new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', ...(task.all_day ? {} : { timeStyle: 'short' as const }), timeZone: task.timezone }).format(task.due_at)}${task.all_day ? ' (All day)' : ''} (${task.timezone})`
}

export function reminderInputDate(task: ReminderTask) {
  const parts = new Intl.DateTimeFormat('en-US', { year: 'numeric', month: '2-digit', day: '2-digit', timeZone: task.timezone }).formatToParts(task.due_at)
  const part = (type: string) => parts.find((item) => item.type === type)!.value
  return `${part('year').padStart(4, '0')}-${part('month')}-${part('day')}`
}

export function normalizeReminders(items: ReminderTask[]) {
  return items.map((item) => ({ ...item,
    expires_at: item.expires_at !== null && item.expires_at > Date.now() ? item.expires_at : null,
    available_today_until: item.available_today_until !== null && item.available_today_until > Date.now() ? item.available_today_until : null,
  }))
}
