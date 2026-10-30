import type { TaskUrgency } from './urgency'

export interface Category {
  id: number
  user_id: number
  description: string
}

export interface Subtask {
  id?: string
  description: string
  completed: boolean
}

export interface Task {
  id: number
  category_id: number
  description: string
  urgency: TaskUrgency
  subtasks: Subtask[]
  completed: boolean
}

export type TaskInput = Omit<Task, 'id' | 'subtasks' | 'completed'>

export interface DailyTask {
  task_id: number
  subtask_id: string
  expires_at: number
}

export interface ScheduledTaskInput {
  description: string
  weekdays: number[]
  timezone: string
  subtasks: Subtask[]
}

export interface ScheduledTask extends ScheduledTaskInput { id: number }

export interface ScheduledOccurrence {
  id: number
  scheduled_task_id: number
  description: string
  subtasks: Subtask[]
  completed: boolean
  expires_at: number
}

export interface ReminderTaskInput {
  description: string
  due_at: number
  timezone: string
}
export interface ReminderTask extends ReminderTaskInput {
  id: number
  completed: boolean
  expires_at: number | null
}
