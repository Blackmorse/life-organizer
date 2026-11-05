import { useId, useState } from 'react'
import { Box, Button, Stack, TextField } from '@mui/material'
import type { Category, TaskInput } from '../types'
import { TASK_URGENCIES, type TaskUrgency } from '../urgency'

interface Props {
  categories: Category[]
  initial?: TaskInput
  preferredCategoryId?: number | null
  disabled: boolean
  submitLabel: string
  onSave: (input: TaskInput) => Promise<boolean>
  onCancel?: () => void
}

export function TaskForm({ categories, initial, preferredCategoryId, disabled, submitLabel, onSave, onCancel }: Props) {
  const id = useId()
  const [description, setDescription] = useState(initial?.description ?? '')
  const [detailedDescription, setDetailedDescription] = useState(initial?.detailed_description ?? '')
  const [urgency, setUrgency] = useState<TaskUrgency>(initial?.urgency ?? 'someday')
  const [categoryId, setCategoryId] = useState(initial?.category_id ?? preferredCategoryId ?? categories[0]?.id ?? 0)
  const resolvedCategoryId = categories.some((category) => category.id === categoryId) ? categoryId : categories[0]?.id ?? 0
  return (
    <Box component="form" onSubmit={async (event) => {
      event.preventDefault()
      if (!description.trim() || !resolvedCategoryId) return
      if (await onSave({ description: description.trim(), detailed_description: detailedDescription, category_id: resolvedCategoryId, urgency }) && !initial) {
        setDescription('')
        setDetailedDescription('')
        setUrgency('someday')
      }
    }}>
      <Stack spacing={2}>
        <TextField id={`${id}-description`} label="Task description" multiline minRows={3} autoFocus
          placeholder="What would you like to work on?" value={description}
          onChange={(event) => setDescription(event.target.value)} required disabled={disabled} />
        <TextField label="Detailed description" multiline minRows={3} value={detailedDescription}
          helperText="Optional. Shown only in expanded Mini App details."
          onChange={(event) => setDetailedDescription(event.target.value)} disabled={disabled} />
        <Stack direction={{ xs: 'column', sm: 'row' }} spacing={2} sx={{ alignItems: { sm: 'center' } }}>
          <TextField id={`${id}-category`} label="Category" select value={resolvedCategoryId}
            slotProps={{ select: { native: true } }} sx={{ flex: 1 }}
            onChange={(event) => setCategoryId(Number(event.target.value))} required disabled={disabled || categories.length === 0}>
            {categories.length === 0 && <option value={0}>Create a category first</option>}
            {categories.map((category) => <option key={category.id} value={category.id}>{category.description}</option>)}
          </TextField>
          <TextField id={`${id}-urgency`} label="Urgency" select value={urgency}
            slotProps={{ select: { native: true } }} sx={{ flex: 1 }}
            helperText={TASK_URGENCIES[urgency].description}
            onChange={(event) => setUrgency(event.target.value as TaskUrgency)} required disabled={disabled}>
            {Object.entries(TASK_URGENCIES).map(([value, option]) => <option key={value} value={value}>{option.label}</option>)}
          </TextField>
          <Stack direction="row" spacing={1} sx={{ flexShrink: 0, justifyContent: 'flex-end' }}>
            {onCancel && <Button onClick={onCancel} disabled={disabled}>Cancel</Button>}
            <Button type="submit" variant="contained" disabled={disabled || !description.trim() || !resolvedCategoryId}>{submitLabel}</Button>
          </Stack>
        </Stack>
      </Stack>
    </Box>
  )
}
