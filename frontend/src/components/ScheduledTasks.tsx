import { useState } from 'react'
import { Box, Button, Checkbox, Collapse, FormControlLabel, FormGroup, Paper, Stack, TextField, Typography } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import type { ScheduledTask, ScheduledTaskInput } from '../types'
import { Subtasks } from './Subtasks'
import { DetailedDescription } from './DetailedDescription'

const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']

function ScheduledForm({ task, disabled, onSave, onCancel }: {
  task?: ScheduledTask, disabled: boolean
  onSave: (input: ScheduledTaskInput, id?: number) => Promise<boolean>
  onCancel: () => void
}) {
  const [editingStep, setEditingStep] = useState(false)
  const [input, setInput] = useState<ScheduledTaskInput>(() => task ? {
    description: task.description, weekdays: task.weekdays,
    detailed_description: task.detailed_description ?? '',
    timezone: task.timezone, subtasks: task.subtasks,
  } : { description: '', detailed_description: '', weekdays: [], timezone: Intl.DateTimeFormat().resolvedOptions().timeZone, subtasks: [] })
  return <Box component="section" aria-label={task ? 'Edit scheduled task' : 'New scheduled task'}>
    <Stack spacing={2}>
      <TextField label="Scheduled task description" autoFocus required multiline value={input.description} disabled={disabled} onChange={(e) => setInput({ ...input, description: e.target.value })} />
      <TextField label="Detailed description" multiline minRows={3} value={input.detailed_description} disabled={disabled} onChange={(e) => setInput({ ...input, detailed_description: e.target.value })} helperText="Optional. Shown only in expanded Mini App details." />
      <Box component="fieldset" sx={{ border: 0, p: 0, m: 0 }} disabled={disabled}>
        <Typography component="legend">Repeat on (required)</Typography>
        <FormGroup row>{WEEKDAYS.map((day, index) => <FormControlLabel key={day} label={day} control={<Checkbox checked={input.weekdays.includes(index + 1)} onChange={(_, checked) => setInput({ ...input, weekdays: checked ? [...input.weekdays, index + 1].sort((a, b) => a - b) : input.weekdays.filter((value) => value !== index + 1) })} />} />)}</FormGroup>
        {!input.weekdays.length && <Typography variant="caption" color="text.secondary">Select at least one weekday. Select all for every day.</Typography>}
      </Box>
      <TextField label="Schedule timezone" required value={input.timezone} disabled={disabled} helperText="IANA timezone, e.g. Europe/Berlin. Each planning day starts at 02:00." onChange={(e) => setInput({ ...input, timezone: e.target.value })} />
      {task && <Typography variant="caption" color="text.secondary">Changes apply to future occurrences. Today’s existing occurrence keeps its saved details and progress.</Typography>}
    </Stack>
    <Subtasks taskId={task?.id ?? 0} subtasks={input.subtasks} disabled={disabled} mode="template" onEditingChange={setEditingStep} onSave={async (subtasks) => { setInput({ ...input, subtasks }); return true }} />
    <Stack direction="row" spacing={1} sx={{ mt: 2 }}>
      <Button variant="contained" disabled={disabled || editingStep || !input.description.trim() || !input.timezone.trim() || !input.weekdays.length} onClick={async () => { if (await onSave(input, task?.id)) onCancel() }}>Save schedule</Button>
      <Button disabled={disabled} onClick={onCancel}>Cancel</Button>
    </Stack>
  </Box>
}

export function ScheduledTasks({ tasks, query, disabled, onSave, onDelete }: {
  tasks: ScheduledTask[], query: string, disabled: boolean
  onSave: (input: ScheduledTaskInput, id?: number) => Promise<boolean>
  onDelete: (id: number) => Promise<boolean>
}) {
  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState<number | null>(null)
  const [expanded, setExpanded] = useState<number | null>(null)
  const visible = tasks.filter((task) => [task.description, ...task.subtasks.map((step) => step.description)].some((text) => text.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())))
  return <Box component="section" aria-label="Scheduled tasks" sx={{ mt: 2 }}>
    <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>Recurring tasks have no category. On due days, the bot offers them at 08:00 in the schedule timezone. Accept the morning offer to add them to Tasks of the day. Completion and removal affect today only.</Typography>
    <Button startIcon={<AddIcon />} disabled={disabled || creating} onClick={() => setCreating(true)}>Add scheduled task</Button>
    {creating && <Paper variant="outlined" sx={{ p: 2, my: 2 }}><ScheduledForm disabled={disabled} onSave={onSave} onCancel={() => setCreating(false)} /></Paper>}
    <Stack spacing={1} sx={{ mt: 2 }}>{visible.map((task) => <Paper key={task.id} variant="outlined" sx={{ p: 2 }}>
      {editing === task.id ? <ScheduledForm task={task} disabled={disabled} onSave={onSave} onCancel={() => setEditing(null)} /> : <>
        <Button color="inherit" aria-label={`View scheduled task ${task.description}`} aria-expanded={expanded === task.id} onClick={() => setExpanded(expanded === task.id ? null : task.id)} sx={{ textAlign: 'left', justifyContent: 'flex-start', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{task.description}</Button>
        <Typography variant="body2" color="text.secondary">{task.weekdays.length === 7 ? 'Every day' : task.weekdays.map((day) => WEEKDAYS[day - 1]).join(', ')} · {task.timezone}</Typography>
        <Collapse in={expanded === task.id} unmountOnExit>
          <DetailedDescription text={task.detailed_description} />
          {task.subtasks.length > 0 && <Box component="ol">{task.subtasks.map((step, index) => <Box component="li" key={index}><Typography sx={{ whiteSpace: 'pre-wrap' }}>{step.description}</Typography><DetailedDescription text={step.detailed_description} /></Box>)}</Box>}
          <Stack direction="row" spacing={1} sx={{ mt: 2 }}>
            <Button disabled={disabled} onClick={() => setEditing(task.id)}>Edit schedule</Button>
            <Button color="error" disabled={disabled} onClick={() => void onDelete(task.id)}>Delete scheduled task</Button>
          </Stack>
        </Collapse>
      </>}
    </Paper>)}</Stack>
    {!visible.length && !creating && <Typography color="text.secondary" sx={{ mt: 2 }}>{query ? 'No matches' : 'No scheduled tasks yet.'}</Typography>}
  </Box>
}
