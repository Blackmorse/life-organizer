export const TASK_URGENCIES = {
  someday: { label: 'Someday', description: 'At some point in the future', color: 'default' },
  upcoming: { label: 'Upcoming', description: 'Within the next few weeks', color: 'info' },
  soon: { label: 'Soon', description: 'Within the next few days', color: 'warning' },
} as const

export type TaskUrgency = keyof typeof TASK_URGENCIES
