import { useState } from 'react'
import { Box, Button, Checkbox, FormControlLabel, IconButton, Stack, TextField, Typography } from '@mui/material'
import ArrowUpwardIcon from '@mui/icons-material/ArrowUpward'
import ArrowDownwardIcon from '@mui/icons-material/ArrowDownward'
import AddIcon from '@mui/icons-material/Add'
import CloseIcon from '@mui/icons-material/Close'
import EditOutlinedIcon from '@mui/icons-material/EditOutlined'
import type { Subtask } from '../types'
import { DailyPickButton } from './DailyTasks'

export function Subtasks({ taskId, subtasks, disabled, readOnly = false, mode = 'task', onSave, onEditingChange }: {
  taskId: number
  subtasks: Subtask[]
  disabled: boolean
  readOnly?: boolean
  mode?: 'task' | 'template' | 'occurrence'
  onEditingChange?: (editing: boolean) => void
  onSave: (steps: Subtask[]) => Promise<boolean>
}) {
  const [draft, updateDraft] = useState<{ index: number | null, description: string } | null>(null)
  function setDraft(next: typeof draft) {
    updateDraft(next)
    onEditingChange?.(next !== null)
  }
  const actionsDisabled = disabled || draft !== null

  function move(index: number, offset: number) {
    const next = [...subtasks]
    const [step] = next.splice(index, 1)
    next.splice(index + offset, 0, step)
    void onSave(next)
  }

  const editor = draft !== null && <Box component="form" sx={{ flex: 1, minWidth: 0, my: 1 }} onSubmit={async (event) => {
    event.preventDefault()
    if (disabled || readOnly || !draft.description.trim()) return
    const next = draft.index === null
      ? [...subtasks, { description: draft.description, completed: false }]
      : subtasks.map((step, index) => index === draft.index ? { ...step, description: draft.description } : step)
    if (await onSave(next)) setDraft(null)
  }}>
    <TextField label="Subtask description" autoFocus required multiline fullWidth value={draft.description} disabled={disabled}
      onChange={(event) => setDraft({ ...draft, description: event.target.value })} />
    <Stack direction="row" spacing={1} sx={{ mt: 1 }}>
      <Button type="submit" variant="contained" size="small" disabled={disabled || !draft.description.trim()}>Save subtask</Button>
      <Button size="small" disabled={disabled} onClick={() => setDraft(null)}>Cancel</Button>
    </Stack>
  </Box>

  return <Box component="section" aria-label="Subtasks" sx={{ mt: 2 }}>
    <Typography variant="h3" sx={{ mb: 1 }}>Subtasks</Typography>
    {subtasks.length > 0 && mode !== 'template' && <Typography variant="caption" color="text.secondary">{subtasks.filter((step) => step.completed).length} of {subtasks.length} completed</Typography>}
    {!readOnly && <Typography variant="caption" color="text.secondary" sx={{ display: 'block' }}>{mode === 'template' ? 'Each scheduled day starts with unchecked subtasks.' : mode === 'occurrence' ? 'Completing every subtask completes today’s occurrence only.' : 'Completing every subtask moves this task to Archive.'}</Typography>}
    {subtasks.length ? <Box component="ol" role="list" sx={{ pl: 0, my: 1, listStyle: 'none', counterReset: 'subtask' }}>
      {subtasks.map((step, index) => <Box component="li" key={index} sx={{
        mb: 0.5, display: 'grid', gridTemplateColumns: '3ch minmax(0, 1fr)', columnGap: 0.5, counterIncrement: 'subtask',
        '&::before': { content: 'counter(subtask) "."', typography: 'body2', color: 'text.secondary', pt: '9px', textAlign: 'right' },
      }}>
        <Stack direction="row" spacing={0.5} sx={{ alignItems: 'flex-start', flexWrap: 'wrap' }}>
          {!readOnly && draft?.index === index ? editor : <FormControlLabel label={step.description} sx={{ flex: 1, minWidth: 0, m: 0, alignItems: 'flex-start' }}
            slotProps={{ typography: { variant: 'body2', sx: { pt: '9px', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', textDecoration: step.completed ? 'line-through' : undefined } } }}
            control={<Checkbox size="small" checked={step.completed} disabled={actionsDisabled || readOnly || mode === 'template'}
              slotProps={{ input: { 'aria-label': `Subtask ${index + 1}: ${step.description}` } }}
              onChange={(_, completed) => void onSave(subtasks.map((item, position) => position === index ? { ...item, completed } : item))} />} />}
          {!readOnly && <Stack direction="row" sx={{ flexShrink: 0 }}>
            {mode === 'task' && <DailyPickButton taskId={taskId} step={step} index={index} label={step.description} disabled={actionsDisabled} />}
            <IconButton size="small" aria-label={`Move subtask ${index + 1} up`} disabled={actionsDisabled || index === 0} onClick={() => move(index, -1)}><ArrowUpwardIcon fontSize="small" /></IconButton>
            <IconButton size="small" aria-label={`Move subtask ${index + 1} down`} disabled={actionsDisabled || index === subtasks.length - 1} onClick={() => move(index, 1)}><ArrowDownwardIcon fontSize="small" /></IconButton>
            <IconButton size="small" aria-label={`Edit subtask ${index + 1}`} disabled={actionsDisabled} onClick={() => setDraft({ index, description: step.description })}><EditOutlinedIcon fontSize="small" /></IconButton>
            <IconButton size="small" aria-label={`Delete subtask ${index + 1}`} disabled={actionsDisabled} onClick={() => void onSave(subtasks.filter((_, position) => position !== index))}><CloseIcon fontSize="small" /></IconButton>
          </Stack>}
        </Stack>
      </Box>)}
    </Box> : <Typography variant="body2" color="text.secondary">No subtasks yet.</Typography>}
    {!readOnly && <>
      <Button size="small" startIcon={<AddIcon />} disabled={actionsDisabled} onClick={() => setDraft({ index: null, description: '' })}>Add subtask</Button>
      {draft?.index === null && editor}
    </>}
  </Box>
}
