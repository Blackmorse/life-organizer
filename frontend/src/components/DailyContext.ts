import { createContext } from 'react'
import type { DailyTask } from '../types'

export const DailyContext = createContext<{
  picks: DailyTask[]
  select: (id: number, index?: number) => Promise<boolean>
  remove: (id: number, stepId: string) => Promise<boolean>
  tomorrowPicks: DailyTask[]
  selectTomorrow: (id: number, index?: number, date?: string) => Promise<boolean>
  removeTomorrow: (id: number, stepId: string) => Promise<boolean>
} | null>(null)
