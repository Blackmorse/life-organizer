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
