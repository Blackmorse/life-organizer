import type { TaskUrgency } from './urgency'

export interface Category {
  id: number
  user_id: number
  description: string
}

export interface Task {
  id: number
  category_id: number
  description: string
  urgency: TaskUrgency
}

export type TaskInput = Omit<Task, 'id'>
