import { useContext, useState } from 'react'
import { Alert, Button, Dialog, DialogActions, DialogContent, DialogContentText, DialogTitle, IconButton, Menu, MenuItem, TextField, Tooltip } from '@mui/material'
import EventOutlinedIcon from '@mui/icons-material/EventOutlined'
import type { DailyTask, Subtask } from '../types'
import { DailyContext } from './DailyContext'
import { nextDate } from '../planning'

export function DailyPickButton({ taskId, step, steps = [], index, label, disabled }: {
  taskId: number, step?: Subtask, steps?: Subtask[], index?: number, label: string, disabled: boolean
}) {
  const daily = useContext(DailyContext)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [open, setOpen] = useState(false)
  const [date, setDate] = useState('')
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  if (!daily) return null
  const selected = (picks: DailyTask[]) => {
    const picked = (id: string | undefined) => id !== undefined && picks.some((item) => item.task_id === taskId && item.subtask_id === id)
    return index === undefined ? picked('') || (steps.length > 0 && steps.every((item) => picked(item.id))) : picked(step?.id)
  }
  const today = selected(daily.picks)
  const tomorrow = selected(daily.tomorrowPicks)
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
  const minimum = nextDate(timezone)
  const busy = disabled || pending
  async function save(action: () => Promise<boolean>) {
    if (disabled || pending) return
    setPending(true)
    setFailed(false)
    try {
      if (await action()) {
        setOpen(false)
        setAnchor(null)
      } else { setFailed(true); setAnchor(null) }
    } finally { setPending(false) }
  }
  return <>
    <Tooltip title="Plan task"><span><IconButton size="small" color={today || tomorrow ? 'primary' : 'default'} aria-label={`Plan task: ${label}`} aria-haspopup="menu" aria-expanded={Boolean(anchor)} disabled={busy} onClick={(event) => setAnchor(event.currentTarget)}><EventOutlinedIcon fontSize="small" /></IconButton></span></Tooltip>
    <Menu anchorEl={anchor} open={Boolean(anchor)} onClose={() => { if (!pending) setAnchor(null) }}>
      <MenuItem disabled={busy} selected={today} onClick={() => void save(() => today ? daily.remove(taskId, step?.id ?? '') : daily.select(taskId, index))}>{today ? 'Remove from today' : 'Add for today'}</MenuItem>
      <MenuItem disabled={busy} selected={tomorrow} onClick={() => void save(() => tomorrow ? daily.removeTomorrow(taskId, step?.id ?? '') : daily.selectTomorrow(taskId, index))}>{tomorrow ? 'Remove from tomorrow' : 'Add for tomorrow'}</MenuItem>
      <MenuItem disabled={busy} onClick={() => { setAnchor(null); setDate(minimum); setFailed(false); setOpen(true) }}>Select date</MenuItem>
    </Menu>
    <Dialog open={open} onClose={() => { if (!busy) setOpen(false) }} fullWidth maxWidth="xs">
      <DialogTitle>Plan task</DialogTitle>
      <DialogContent>
        {failed && <Alert severity="error" sx={{ mb: 2 }}>Could not plan this task. Your selected date is kept. Retry, or cancel to review the workspace error.</Alert>}
        <DialogContentText sx={{ mb: 2 }}>{label}. The bot will offer it at 08:00 on the chosen date ({timezone}). Today's selection and progress stay saved.</DialogContentText>
        <TextField label="Target date" type="date" fullWidth required value={date} disabled={busy} onChange={(event) => setDate(event.target.value)} slotProps={{ inputLabel: { shrink: true }, htmlInput: { min: minimum, max: '9998-12-31' } }} />
      </DialogContent>
      <DialogActions>
        <Button disabled={busy} onClick={() => setOpen(false)}>Cancel</Button>
        <Button disabled={busy || date < minimum || date > '9998-12-31'} onClick={() => void save(() => daily.selectTomorrow(taskId, index, date))}>Add</Button>
      </DialogActions>
    </Dialog>
  </>
}
