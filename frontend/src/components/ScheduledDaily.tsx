import { useState } from 'react'
import { Checkbox, Chip, Collapse, IconButton, ListItem, Stack, Tooltip, Typography } from '@mui/material'
import CloseIcon from '@mui/icons-material/Close'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import ExpandLessIcon from '@mui/icons-material/ExpandLess'
import type { ScheduledOccurrence, Subtask } from '../types'
import { Subtasks } from './Subtasks'
import { DetailedDescription } from './DetailedDescription'

export function ScheduledDaily({ item, disabled, onComplete, onSubtasks, onDismiss }: {
  item: ScheduledOccurrence, disabled: boolean
  onComplete: (id: number, completed: boolean) => Promise<boolean>
  onSubtasks: (id: number, steps: Subtask[]) => Promise<boolean>
  onDismiss: (id: number) => Promise<boolean>
}) {
  const [expanded, setExpanded] = useState(false)
  return <ListItem disableGutters sx={{ display: 'block' }}>
    <Stack direction="row" spacing={1} sx={{ alignItems: 'flex-start' }}>
      <Checkbox checked={item.completed} disabled={disabled} slotProps={{ input: { 'aria-label': `Complete daily ${item.description}` } }} onChange={(_, completed) => void onComplete(item.id, completed)} />
      <Stack sx={{ flex: 1, minWidth: 0, pt: 1 }}>
        <Typography sx={{ fontWeight: 600, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', textDecoration: item.completed ? 'line-through' : undefined }}>{item.description}</Typography>
        <Chip label="Scheduled" size="small" variant="outlined" sx={{ alignSelf: 'flex-start', mt: 0.5 }} />
      </Stack>
      <IconButton aria-label={`View daily scheduled task ${item.description}`} aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? <ExpandLessIcon /> : <ExpandMoreIcon />}</IconButton>
      <Tooltip title="Skip today only"><span><IconButton aria-label={`Remove from Tasks of the day: ${item.description}`} disabled={disabled} onClick={() => void onDismiss(item.id)}><CloseIcon /></IconButton></span></Tooltip>
    </Stack>
    <Collapse in={expanded} unmountOnExit sx={{ ml: 3 }}>
      <DetailedDescription text={item.detailed_description} />
      <Subtasks taskId={item.id} subtasks={item.subtasks} disabled={disabled} mode="occurrence" onSave={(steps) => onSubtasks(item.id, steps)} />
    </Collapse>
  </ListItem>
}
