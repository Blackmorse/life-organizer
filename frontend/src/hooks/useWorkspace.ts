import { useEffect, useRef, useState } from 'react'
import { api, AuthorizationError } from '../api'
import type { Category, DailyTask, Subtask, Task, TaskInput } from '../types'

export function useWorkspace() {
  const [categories, setCategories] = useState<Category[]>([])
  const [tasks, setTasks] = useState<Task[]>([])
  const [dailyTasks, setDailyTasks] = useState<DailyTask[]>([])
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [authorizationRequired, setAuthorizationRequired] = useState(false)
  const [revision, setRevision] = useState(0)
  const mutationInFlight = useRef(false)

  useEffect(() => {
    const controller = new AbortController()
    Promise.all([api.listCategories(controller.signal), api.listTasks(controller.signal), api.listDailyTasks(controller.signal)])
      .then(([nextCategories, nextTasks, nextDailyTasks]) => {
        if (controller.signal.aborted) return
        setCategories(nextCategories)
        setTasks(nextTasks)
        setDailyTasks(nextDailyTasks.filter((item) => item.expires_at > Date.now()))
        setAuthorizationRequired(false)
      })
      .catch((cause: unknown) => {
        if (!controller.signal.aborted) {
          setError(errorMessage(cause))
          setAuthorizationRequired(cause instanceof AuthorizationError)
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false)
      })
    return () => controller.abort()
  }, [revision])

  useEffect(() => {
    function expire() { setDailyTasks((items) => items.filter((item) => item.expires_at > Date.now())) }
    const nextExpiry = Math.min(...dailyTasks.map((item) => item.expires_at))
    const timer = Number.isFinite(nextExpiry) ? window.setTimeout(expire, Math.max(0, nextExpiry - Date.now())) : undefined
    window.addEventListener('focus', expire)
    document.addEventListener('visibilitychange', expire)
    return () => {
      window.clearTimeout(timer)
      window.removeEventListener('focus', expire)
      document.removeEventListener('visibilitychange', expire)
    }
  }, [dailyTasks])

  async function mutate(operation: () => Promise<void>): Promise<boolean> {
    if (mutationInFlight.current || loading || authorizationRequired) return false
    mutationInFlight.current = true
    setSaving(true)
    setError(null)
    try {
      await operation()
      return true
    } catch (cause) {
      setError(errorMessage(cause))
      setAuthorizationRequired(cause instanceof AuthorizationError)
      return false
    } finally {
      mutationInFlight.current = false
      setSaving(false)
    }
  }

  async function applyCompletion(task: Task) {
    const previous = tasks.find((item) => item.id === task.id)
    setTasks((items) => items.map((item) => item.id === task.id ? task : item))
    const pickCompleted = (item: Task, stepId: string) => stepId
      ? item.subtasks.some((step) => step.id === stepId && (item.completed || step.completed))
      : item.completed
    const dailyProgressChanged = previous && dailyTasks.some((pick) =>
      pick.task_id === task.id && pick.expires_at > Date.now()
      && (!pick.subtask_id || task.subtasks.some((step) => step.id === pick.subtask_id))
      && pickCompleted(previous, pick.subtask_id) !== pickCompleted(task, pick.subtask_id))
    if (!dailyProgressChanged) return
    try {
      await api.finalizeDailyTasks()
    } catch (cause) {
      // Completion is already saved. Report delivery separately; never undo it.
      setError(`Progress saved, but the Telegram summary could not be sent. ${errorMessage(cause)} Use Finalize tasks to resend the current list.`)
      setAuthorizationRequired(cause instanceof AuthorizationError)
    }
  }

  return {
    categories, tasks, dailyTasks, loading, saving, error, authorizationRequired,
    finalizeDailyTasks: () => mutate(async () => { await api.finalizeDailyTasks() }),
    selectDailyTask: (id: number, index?: number) => mutate(async () => {
      const picks = await api.selectDailyTask(id, index)
      const nextTasks = await api.listTasks()
      setTasks(nextTasks)
      setDailyTasks(picks.filter((item) => item.expires_at > Date.now()))
    }),
    removeDailyTask: (id: number, stepId: string) => mutate(async () => {
      setDailyTasks(await api.removeDailyTask(id, stepId))
    }),
    refresh: () => {
      if (mutationInFlight.current || loading) return
      setLoading(true)
      setError(null)
      setRevision((value) => value + 1)
    },
    createCategory: (description: string) => mutate(async () => {
      const category = await api.createCategory(description)
      setCategories((items) => [...items, category])
    }),
    updateCategory: (id: number, description: string) => mutate(async () => {
      const category = await api.updateCategory(id, description)
      setCategories((items) => items.map((item) => item.id === id ? category : item))
    }),
    deleteCategory: (id: number) => mutate(async () => {
      await api.deleteCategory(id)
      setCategories((items) => items.filter((item) => item.id !== id))
    }),
    createTask: (input: TaskInput) => mutate(async () => {
      const task = await api.createTask(input)
      setTasks((items) => [...items, task])
    }),
    updateTask: (id: number, input: TaskInput) => mutate(async () => {
      const task = await api.updateTask(id, input)
      setTasks((items) => items.map((item) => item.id === id ? task : item))
    }),
    deleteTask: (id: number) => mutate(async () => {
      await api.deleteTask(id)
      setTasks((items) => items.filter((item) => item.id !== id))
    }),
    replaceSubtasks: (id: number, subtasks: Subtask[]) => mutate(async () => {
      const task = await api.replaceSubtasks(id, subtasks)
      await applyCompletion(task)
    }),
    setCompleted: (id: number, completed: boolean) => mutate(async () => {
      const task = await api.setCompleted(id, completed)
      await applyCompletion(task)
    }),
  }
}

function errorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : 'Something went wrong. Please retry.'
}
