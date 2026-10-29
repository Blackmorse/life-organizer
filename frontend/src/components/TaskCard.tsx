import { useState } from 'react'
import { Box, Button, ButtonBase, Chip, ListItem, Paper, Stack, Typography } from '@mui/material'
import ChevronRightIcon from '@mui/icons-material/ChevronRight'
import DeleteOutlineIcon from '@mui/icons-material/DeleteOutlined'
import EditOutlinedIcon from '@mui/icons-material/EditOutlined'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import type { Category, Task, TaskInput } from '../types'
import { TaskForm } from './TaskForm'
import { TASK_URGENCIES } from '../urgency'

export function TaskCard({ task, categories, disabled, onUpdate, onDelete }: {
  task: Task
  categories: Category[]
  disabled: boolean
  onUpdate: (id: number, input: TaskInput) => Promise<boolean>
  onDelete: (id: number) => Promise<boolean>
}) {
  const [editing, setEditing] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const category = categories.find((item) => item.id === task.category_id)
  const urgency = TASK_URGENCIES[task.urgency]
  return (
    <ListItem disablePadding sx={{ display: 'block' }}>
      <ButtonBase aria-label={`View task ${task.description}`} aria-expanded={expanded} aria-controls={`task-${task.id}-details`}
        onClick={() => setExpanded(!expanded)} sx={{ display: 'flex', width: '100%', minHeight: 44, p: 1, gap: 1, textAlign: 'left', borderRadius: 1, '&:hover': { bgcolor: 'action.hover' } }}>
        {expanded ? <ExpandMoreIcon fontSize="small" color="action" /> : <ChevronRightIcon fontSize="small" color="action" />}
        <Typography component="span" variant="body2" noWrap sx={{ flex: 1, minWidth: 0 }}>{task.description}</Typography>
        <Chip component="span" size="small" variant="outlined" label={urgency.label} color={urgency.color} title={urgency.description} />
        <Typography component="span" variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>#{task.id}</Typography>
      </ButtonBase>
      <Box id={`task-${task.id}-details`} hidden={!expanded} sx={{ display: expanded ? 'block' : 'none', ml: { xs: 0, sm: 3 }, mb: 1 }}>
        <Paper variant="outlined" sx={{ p: 2 }}>
          {editing ? (
            <TaskForm categories={categories} initial={task} disabled={disabled} submitLabel="Save changes"
              onCancel={() => setEditing(false)} onSave={async (input) => {
                const saved = await onUpdate(task.id, input)
                if (saved) setEditing(false)
                return saved
              }} />
          ) : (
            <>
              <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', lineHeight: 1.7 }}>{task.description}</Typography>
              <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>Urgency: {urgency.label} — {urgency.description}</Typography>
              <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5 }}>In {category?.description ?? `Category #${task.category_id}`}</Typography>
              <Stack direction="row" spacing={1} sx={{ mt: 1 }}>
                <Button size="small" startIcon={<EditOutlinedIcon />} disabled={disabled} aria-label={`Edit task ${task.description}`} onClick={() => setEditing(true)}>Edit / move</Button>
                <Button size="small" color="error" startIcon={<DeleteOutlineIcon />} disabled={disabled} aria-label={`Delete task ${task.description}`} onClick={() => void onDelete(task.id)}>Delete</Button>
              </Stack>
            </>
          )}
        </Paper>
      </Box>
    </ListItem>
  )
}
