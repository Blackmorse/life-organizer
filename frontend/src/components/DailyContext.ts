import { createContext } from 'react'
import type { DailyTask } from '../types'

export const DailyContext = createContext<{
  picks: DailyTask[]
  select: (id: number, index?: number) => Promise<boolean>
  remove: (id: number, stepId: string) => Promise<boolean>
} | null>(null)
