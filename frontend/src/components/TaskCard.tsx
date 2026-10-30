import { useState } from 'react'
import { Box, Button, ButtonBase, Chip, ListItem, Paper, Stack, Typography } from '@mui/material'
import ChevronRightIcon from '@mui/icons-material/ChevronRight'
import DeleteOutlineIcon from '@mui/icons-material/DeleteOutlined'
import EditOutlinedIcon from '@mui/icons-material/EditOutlined'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import CheckCircleOutlinedIcon from '@mui/icons-material/CheckCircleOutlined'
import RestoreIcon from '@mui/icons-material/Restore'
import type { Category, Subtask, Task, TaskInput } from '../types'
import { TaskForm } from './TaskForm'
import { TASK_URGENCIES } from '../urgency'
import { Subtasks } from './Subtasks'
import { DailyPickButton } from './DailyTasks'

export function TaskCard({ task, categories, disabled, onUpdate, onDelete, onReplaceSubtasks, onSetCompleted }: {
  task: Task
  categories: Category[]
  disabled: boolean
  onUpdate: (id: number, input: TaskInput) => Promise<boolean>
  onDelete: (id: number) => Promise<boolean>
  onReplaceSubtasks: (id: number, subtasks: Subtask[]) => Promise<boolean>
  onSetCompleted: (id: number, completed: boolean) => Promise<boolean>
}) {
  const [editing, setEditing] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const category = categories.find((item) => item.id === task.category_id)
  const urgency = TASK_URGENCIES[task.urgency]
  return (
    <ListItem disablePadding sx={{ display: 'block' }}>
      <Stack direction="row" sx={{ alignItems: 'center' }}>
      <ButtonBase aria-label={`View task ${task.description}`} aria-expanded={expanded} aria-controls={`task-${task.id}-details`}
        onClick={() => setExpanded(!expanded)} sx={{ display: 'flex', flex: 1, minWidth: 0, minHeight: 44, p: 1, gap: 1, textAlign: 'left', borderRadius: 1, '&:hover': { bgcolor: 'action.hover' } }}>
        {expanded ? <ExpandMoreIcon fontSize="small" color="action" /> : <ChevronRightIcon fontSize="small" color="action" />}
        <Typography component="span" variant="body2" noWrap sx={{ flex: 1, minWidth: 0 }}>{task.description}</Typography>
        <Chip component="span" size="small" variant="outlined" label={urgency.label} color={urgency.color} title={urgency.description} />
        {task.subtasks.length > 0 && <Typography component="span" variant="caption" color="text.secondary">{task.subtasks.filter((step) => step.completed).length}/{task.subtasks.length} steps</Typography>}
        <Typography component="span" variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>#{task.id}</Typography>
      </ButtonBase>
      {!task.completed && <DailyPickButton taskId={task.id} steps={task.subtasks} label={task.description} disabled={disabled} />}
      </Stack>
      <Box id={`task-${task.id}-details`} hidden={!expanded} sx={{ display: expanded ? 'block' : 'none', ml: { xs: 0, sm: 3 }, mb: 1 }}>
        <Paper variant="outlined" sx={{ p: 2 }}>
          {editing && !task.completed ? (
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
              <Stack direction="row" spacing={1} sx={{ mt: 1, flexWrap: 'wrap', gap: 1 }}>
                {task.completed ? <Button size="small" startIcon={<RestoreIcon />} disabled={disabled} aria-label={`Restore task ${task.description}`} onClick={() => void onSetCompleted(task.id, false)}>Restore</Button> : <>
                <Button size="small" color="success" startIcon={<CheckCircleOutlinedIcon />} disabled={disabled} aria-label={`Complete task ${task.description}`} onClick={() => void onSetCompleted(task.id, true)}>Complete</Button>
                <Button size="small" startIcon={<EditOutlinedIcon />} disabled={disabled} aria-label={`Edit task ${task.description}`} onClick={() => setEditing(true)}>Edit / move</Button>
                <Button size="small" color="error" startIcon={<DeleteOutlineIcon />} disabled={disabled} aria-label={`Delete task ${task.description}`} onClick={() => void onDelete(task.id)}>Delete</Button>
                </>}
              </Stack>
              <Subtasks taskId={task.id} subtasks={task.subtasks} disabled={disabled} readOnly={task.completed} onSave={(steps) => onReplaceSubtasks(task.id, steps)} />
            </>
          )}
        </Paper>
      </Box>
    </ListItem>
  )
}
