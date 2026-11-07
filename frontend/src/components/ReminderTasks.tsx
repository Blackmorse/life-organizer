import { useState } from 'react'
import { Box, Button, Checkbox, Chip, IconButton, List, ListItem, Paper, Stack, TextField, Typography } from '@mui/material'
import CloseIcon from '@mui/icons-material/Close'
import type { ReminderTask, ReminderTaskInput } from '../types'
import { reminderDate, reminderInputDate } from '../reminders'
import { DetailedDescriptionDisclosure } from './DetailedDescription'
import { MoveDailyButton, type MoveDaily } from './MoveDailyButton'

function localInput(instant: number) {
  const date = new Date(instant)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}T${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

function ReminderForm({ task, disabled, onSave, onCancel }: {
  task?: ReminderTask, disabled: boolean
  onSave: (input: ReminderTaskInput, id?: number) => Promise<boolean>
  onCancel: () => void
}) {
  const [description, setDescription] = useState(task?.description ?? '')
  const [detailedDescription, setDetailedDescription] = useState(task?.detailed_description ?? '')
  const initialDateTime = task ? localInput(task.due_at) : ''
  const initialDate = task?.all_day ? reminderInputDate(task) : initialDateTime.split('T')[0]
  const initialTime = task && !task.all_day ? initialDateTime.split('T')[1] : ''
  const [date, setDate] = useState(initialDate)
  const [time, setTime] = useState(initialTime)
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
  const unchangedDate = task && initialDate === date && initialTime === time
  const dueAt = new Date(`${date}T${time || '00:00'}`).getTime()
  return <Stack component="form" spacing={2} onSubmit={async (event) => {
    event.preventDefault()
    if (await onSave({ description: description.trim(), detailed_description: detailedDescription, due_at: unchangedDate ? task.due_at : dueAt, all_day: !time, timezone: unchangedDate ? task.timezone : timezone }, task?.id)) onCancel()
  }}>
    <TextField label="Reminder description" required autoFocus value={description} disabled={disabled} onChange={(event) => setDescription(event.target.value)} />
    <TextField label="Detailed description" multiline minRows={3} value={detailedDescription} disabled={disabled} onChange={(event) => setDetailedDescription(event.target.value)} helperText="Optional. Shown only in expanded Mini App details." />
    <TextField label="Reminder date" type="date" required value={date} disabled={disabled} onChange={(event) => setDate(event.target.value)} slotProps={{ inputLabel: { shrink: true }, htmlInput: { min: '1970-01-01', max: '9998-12-31' } }} helperText={`Device timezone: ${timezone}. Existing reminder: ${task ? reminderDate(task) : 'new'}.`} />
    <TextField label="Time (optional)" type="time" value={time} disabled={disabled} onChange={(event) => setTime(event.target.value)} slotProps={{ inputLabel: { shrink: true } }} helperText="Leave empty for a task to do anytime during this day." />
    <Stack direction="row" spacing={1}>
      <Button type="submit" variant="contained" disabled={disabled || !description.trim() || !Number.isFinite(dueAt)}>Save reminder</Button>
      <Button disabled={disabled} onClick={onCancel}>Cancel</Button>
    </Stack>
  </Stack>
}

export function ReminderTasks({ tasks, query, disabled, onSave, onDelete, onComplete, onSelectToday }: {
  tasks: ReminderTask[], query: string, disabled: boolean
  onSave: (input: ReminderTaskInput, id?: number) => Promise<boolean>
  onDelete: (id: number) => Promise<boolean>
  onComplete: (id: number, completed: boolean) => Promise<boolean>
  onSelectToday: (id: number) => Promise<boolean>
}) {
  const [editing, setEditing] = useState<number | 'new' | null>(null)
  const visible = tasks.filter((item) => `${item.description} ${reminderDate(item)}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).sort((a, b) => a.due_at - b.due_at || a.id - b.id)
  return <Box component="section" aria-label="Reminder tasks" sx={{ mt: 2 }}>
    <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>One-time tasks with a date and optional time. Leave time empty to do the task anytime during that day. The bot gives a heads-up at 08:00 the day before, then offers them at 08:00 on their date, in the saved timezone. Use Add to today for an unfinished reminder due today, even if you skipped the offer.</Typography>
    <Button disabled={disabled || editing !== null} onClick={() => setEditing('new')}>Add reminder task</Button>
    {editing === 'new' && <Paper variant="outlined" sx={{ p: 2, mt: 2 }}><ReminderForm disabled={disabled} onSave={onSave} onCancel={() => setEditing(null)} /></Paper>}
    <List>{visible.map((task) => <ListItem key={task.id} disableGutters sx={{ display: 'block' }}>
      {editing === task.id ? <ReminderForm task={task} disabled={disabled} onSave={onSave} onCancel={() => setEditing(null)} /> : <Paper variant="outlined" sx={{ p: 2 }}>
        <Typography sx={{ fontWeight: 600, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', textDecoration: task.completed ? 'line-through' : undefined }}>{task.description}</Typography>
        <Typography variant="body2" color="text.secondary">{reminderDate(task)}</Typography>
        <DetailedDescriptionDisclosure text={task.detailed_description} label={task.description} />
        {task.completed && <Chip label="Completed" size="small" sx={{ mt: 1 }} />}
        {task.expires_at !== null ? <Chip label="In Tasks of the day" size="small" color="primary" sx={{ mt: 1 }} /> : !task.completed && task.available_today_until !== null && <Button aria-label={`Add to today: ${task.description}`} disabled={disabled} onClick={() => void onSelectToday(task.id)}>Add to today</Button>}
        <Stack direction="row" spacing={1} sx={{ mt: 1 }}>
          <Button disabled={disabled || editing !== null} onClick={() => setEditing(task.id)}>Edit reminder</Button>
          <Button disabled={disabled} onClick={() => void onComplete(task.id, !task.completed)}>{task.completed ? 'Reopen reminder' : 'Complete reminder'}</Button>
          <Button color="error" disabled={disabled} onClick={() => void onDelete(task.id)}>Delete reminder</Button>
        </Stack>
      </Paper>}
    </ListItem>)}</List>
    {!visible.length && <Typography color="text.secondary">{query ? 'No matches' : 'No reminder tasks yet.'}</Typography>}
  </Box>
}

export function ReminderDaily({ task, disabled, onComplete, onDismiss, onMove }: {
  onMove: MoveDaily
  task: ReminderTask, disabled: boolean
  onComplete: (id: number, completed: boolean) => Promise<boolean>
  onDismiss: (id: number) => Promise<boolean>
}) {
  return <ListItem disableGutters sx={{ alignItems: 'flex-start', gap: 1 }}>
    <Checkbox checked={task.completed} disabled={disabled} slotProps={{ input: { 'aria-label': `Complete daily ${task.description}` } }} onChange={(_, completed) => void onComplete(task.id, completed)} />
    <Stack sx={{ flex: 1, minWidth: 0, pt: 1 }}>
      <Typography sx={{ fontWeight: 600, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', textDecoration: task.completed ? 'line-through' : undefined }}>{task.description}</Typography>
      <Typography variant="caption" color="text.secondary">{reminderDate(task)}</Typography>
      <DetailedDescriptionDisclosure text={task.detailed_description} label={task.description} />
      <Chip label="Reminder" size="small" variant="outlined" sx={{ alignSelf: 'flex-start', mt: 0.5 }} />
    </Stack>
    {!task.completed && <MoveDailyButton id={`reminder:${task.id}`} label={task.description} disabled={disabled} onMove={onMove} timezone={task.timezone} reminder />}
    <IconButton aria-label={`Remove from Tasks of the day: ${task.description}`} disabled={disabled} onClick={() => void onDismiss(task.id)}><CloseIcon /></IconButton>
  </ListItem>
}
