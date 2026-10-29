import { useContext, useState } from 'react'
import { Alert, Box, Button, Checkbox, IconButton, List, ListItem, Stack, Tooltip, Typography } from '@mui/material'
import TodayOutlinedIcon from '@mui/icons-material/TodayOutlined'
import CloseIcon from '@mui/icons-material/Close'
import SendOutlinedIcon from '@mui/icons-material/SendOutlined'
import type { Category, DailyTask, Subtask, Task } from '../types'

import { DailyContext } from './DailyContext'

export function DailyPickButton({ taskId, step, index, label, disabled }: {
  taskId: number, step?: Subtask, index?: number, label: string, disabled: boolean
}) {
  const daily = useContext(DailyContext)
  if (!daily) return null
  const selected = daily.picks.some((item) => item.task_id === taskId && item.subtask_id === (index === undefined ? '' : step?.id))
  const action = `${selected ? 'Remove from' : 'Add to'} Tasks of the day: ${label}`
  return <Tooltip title={action}><span><IconButton size="small" color={selected ? 'primary' : 'default'} aria-label={action} aria-pressed={selected} disabled={disabled}
    onClick={() => void (selected ? daily.remove(taskId, step?.id ?? '') : daily.select(taskId, index))}>
    <TodayOutlinedIcon fontSize="small" />
  </IconButton></span></Tooltip>
}

export function DailyTasks({ picks, tasks, categories, query, disabled, onFinalize, onRemove, onSetCompleted, onReplaceSubtasks }: {
  picks: DailyTask[], tasks: Task[], categories: Category[], query: string, disabled: boolean
  onRemove: (id: number, stepId: string) => Promise<boolean>
  onSetCompleted: (id: number, completed: boolean) => Promise<boolean>
  onReplaceSubtasks: (id: number, steps: Subtask[]) => Promise<boolean>
  onFinalize: () => Promise<boolean>
}) {
  const [sent, setSent] = useState<{ picks: DailyTask[], tasks: Task[] } | null>(null)
  const rows = picks.flatMap((pick) => {
    const task = tasks.find((item) => item.id === pick.task_id)
    if (!task) return []
    const step = pick.subtask_id ? task.subtasks.find((item) => item.id === pick.subtask_id) : undefined
    if (pick.subtask_id && !step) return []
    const category = categories.find((item) => item.id === task.category_id)?.description ?? ''
    const description = step?.description ?? task.description
    return [{ pick, task, step, category, description }]
  })
  const visibleRows = rows.filter(({ description, task, category }) =>
    [description, task.description, category].some((text) => text.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())))
  const groups = new Map<number, typeof rows>()
  for (const row of visibleRows) {
    const group = groups.get(row.task.id)
    if (group) group.push(row)
    else groups.set(row.task.id, [row])
  }
  const renderPick = ({ pick, task, step, category, description }: typeof rows[number]) =>
    <ListItem key={pick.subtask_id} component={step ? 'li' : 'div'} disableGutters sx={{ alignItems: 'flex-start', gap: 1, ...(step && { py: 0 }) }}>
      <Checkbox checked={task.completed || (step?.completed ?? false)} disabled={disabled || Boolean(step && task.completed)}
        slotProps={{ input: { 'aria-label': `Complete daily ${description}` } }}
        onChange={(_, completed) => void (step
          ? onReplaceSubtasks(task.id, task.subtasks.map((item) => item.id === step.id ? { ...item, completed } : item))
          : onSetCompleted(task.id, completed))} />
      <Stack sx={{ flex: 1, minWidth: 0, pt: 1 }}>
        <Typography sx={{ fontWeight: step ? undefined : 600, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', textDecoration: task.completed || step?.completed ? 'line-through' : undefined }}>{description}</Typography>
        {!step && <Typography variant="caption" color="text.secondary">{category}</Typography>}
      </Stack>
      <IconButton aria-label={`Remove from Tasks of the day: ${description}`} disabled={disabled} onClick={() => void onRemove(task.id, pick.subtask_id)}><CloseIcon /></IconButton>
    </ListItem>
  return <Box component="section" aria-label="Daily checklist" sx={{ mt: 2 }}>
    <Typography variant="body2" color="text.secondary">Clears at 2:00 AM ({Intl.DateTimeFormat().resolvedOptions().timeZone}). Tasks and progress stay saved.</Typography>
    <Stack spacing={1} sx={{ my: 2, alignItems: 'flex-start' }}>
      <Button variant="contained" startIcon={<SendOutlinedIcon />} disabled={disabled || rows.length === 0} onClick={async () => {
        setSent(null)
        if (await onFinalize()) setSent({ picks, tasks })
      }}>Finalize tasks</Button>
      <Typography variant="caption" color="text.secondary">Sends all current daily picks to your Telegram bot chat. Completion changes automatically send an updated list.</Typography>
    </Stack>
    {sent?.picks === picks && sent.tasks === tasks && <Alert severity="success" role="status">Today's tasks sent to your Telegram bot chat.</Alert>}
    <List>
      {[...groups.entries()].map(([taskId, group]) => {
        const { task, category } = group[0]
        const wholeTask = group.find(({ step }) => !step)
        const steps = group.filter(({ step }) => step)
        return <ListItem key={taskId} disableGutters sx={{ display: 'block' }}>
          {wholeTask ? renderPick(wholeTask) : <Stack sx={{ px: 1.5, py: 1 }}>
            <Typography sx={{ fontWeight: 600, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{task.description}</Typography>
            <Typography variant="caption" color="text.secondary">{category}</Typography>
          </Stack>}
          {steps.length > 0 && <List disablePadding aria-label={`Selected subtasks for ${task.description}`} sx={{ ml: { xs: 2, sm: 4 }, pl: 1, borderLeft: 1, borderColor: 'divider' }}>
            {steps.map(renderPick)}
          </List>}
        </ListItem>
      })}
    </List>
    {!visibleRows.length && <Typography color="text.secondary">{query ? 'No matches' : 'No tasks of the day yet. Use the calendar buttons beside tasks or subtasks to add them.'}</Typography>}
  </Box>
}
