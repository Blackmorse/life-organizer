import { useEffect, useRef, useState } from 'react'
import { api, AuthorizationError } from '../api'
import type { Category, Task, TaskInput } from '../types'

export function useWorkspace() {
  const [categories, setCategories] = useState<Category[]>([])
  const [tasks, setTasks] = useState<Task[]>([])
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [authorizationRequired, setAuthorizationRequired] = useState(false)
  const [revision, setRevision] = useState(0)
  const mutationInFlight = useRef(false)

  useEffect(() => {
    const controller = new AbortController()
    Promise.all([api.listCategories(controller.signal), api.listTasks(controller.signal)])
      .then(([nextCategories, nextTasks]) => {
        if (controller.signal.aborted) return
        setCategories(nextCategories)
        setTasks(nextTasks)
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

  return {
    categories, tasks, loading, saving, error, authorizationRequired,
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
  }
}

function errorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : 'Something went wrong. Please retry.'
}
