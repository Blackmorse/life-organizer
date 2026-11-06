import { useState } from 'react'
import { Alert, Button, Dialog, DialogActions, DialogContent, DialogContentText, DialogTitle, IconButton, Menu, MenuItem, TextField, Tooltip } from '@mui/material'
import EventRepeatIcon from '@mui/icons-material/EventRepeat'
import { nextDate } from '../planning'

export type MoveDaily = (id: string, date: string) => Promise<boolean>

export function MoveDailyButton({ id, label, disabled, onMove, timezone = Intl.DateTimeFormat().resolvedOptions().timeZone, reminder = false }: {
  id: string, label: string, disabled: boolean, onMove: MoveDaily, timezone?: string, reminder?: boolean
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [open, setOpen] = useState(false)
  const [date, setDate] = useState('')
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const minimum = nextDate(timezone)
  async function move(target: string) {
    if (pending || disabled) return
    setPending(true)
    setFailed(false)
    try {
      if (await onMove(id, target)) { setOpen(false); setAnchor(null) }
      else { setFailed(true); setAnchor(null) }
    } finally { setPending(false) }
  }
  return <>
    <Tooltip title="Move to another day"><span><IconButton aria-label={`Move to another day: ${label}`} disabled={disabled || pending} onClick={(event) => setAnchor(event.currentTarget)}><EventRepeatIcon /></IconButton></span></Tooltip>
    <Menu anchorEl={anchor} open={Boolean(anchor)} onClose={() => { if (!pending) setAnchor(null) }}>
      <MenuItem disabled={disabled || pending} onClick={() => void move('tomorrow')}>Tomorrow</MenuItem>
      <MenuItem disabled={disabled || pending} onClick={() => { setAnchor(null); setDate(minimum); setFailed(false); setOpen(true) }}>Select date</MenuItem>
    </Menu>
    <Dialog open={open} onClose={() => { if (!disabled && !pending) setOpen(false) }} fullWidth maxWidth="xs">
      <DialogTitle>Move to another day</DialogTitle>
      <DialogContent>
        {failed && <Alert severity="error" sx={{ mb: 2 }}>Could not move this task. Your selected date is kept. Retry, or cancel to review the workspace error.</Alert>}
        <DialogContentText sx={{ mb: 2 }}>{label}. {reminder ? 'Reschedules this reminder, keeping its local time.' : 'Removes selected work from today, preserving progress.'} The bot will offer it at 08:00 on the chosen date ({timezone}).</DialogContentText>
        <TextField label="Target date" type="date" fullWidth required value={date} disabled={disabled || pending} onChange={(event) => setDate(event.target.value)} slotProps={{ inputLabel: { shrink: true }, htmlInput: { min: minimum, max: '9998-12-31' } }} />
      </DialogContent>
      <DialogActions>
        <Button disabled={disabled || pending} onClick={() => setOpen(false)}>Cancel</Button>
        <Button disabled={disabled || pending || date < minimum || date > '9998-12-31'} onClick={() => void move(date)}>Move</Button>
      </DialogActions>
    </Dialog>
  </>
}
