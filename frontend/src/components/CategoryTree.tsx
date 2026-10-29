import { useState } from 'react'
import { Box, Button, ButtonBase, Chip, IconButton, List, ListItem, ListItemIcon, ListItemText, Menu, MenuItem, Paper, Stack, Typography } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import ChevronRightIcon from '@mui/icons-material/ChevronRight'
import DeleteOutlineIcon from '@mui/icons-material/DeleteOutlined'
import EditOutlinedIcon from '@mui/icons-material/EditOutlined'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import FolderOutlinedIcon from '@mui/icons-material/FolderOutlined'
import MoreHorizIcon from '@mui/icons-material/MoreHoriz'
import type { Category, Subtask, Task, TaskInput } from '../types'
import { CategoryForm } from './CategoryForm'
import { TaskCard } from './TaskCard'
import { TaskForm } from './TaskForm'

interface Props {
  categories: Category[]
  tasks: Task[]
  query: string
  disabled: boolean
  archived: boolean
  onUpdateCategory: (id: number, description: string) => Promise<boolean>
  onDeleteCategory: (id: number) => Promise<boolean>
  onCreateTask: (input: TaskInput) => Promise<boolean>
  onUpdateTask: (id: number, input: TaskInput) => Promise<boolean>
  onDeleteTask: (id: number) => Promise<boolean>
  onReplaceSubtasks: (id: number, subtasks: Subtask[]) => Promise<boolean>
  onSetCompleted: (id: number, completed: boolean) => Promise<boolean>
}

export function CategoryTree(props: Props) {
  const [collapsedIds, setCollapsedIds] = useState<Set<number>>(() => new Set())
  const query = props.query.trim().toLocaleLowerCase()
  const branches = props.categories.map((category) => {
    const tasks = props.tasks.filter((task) => task.category_id === category.id)
    const matchesCategory = category.description.toLocaleLowerCase().includes(query)
    const matchingTasks = matchesCategory ? tasks : tasks.filter((task) => [task.description, ...task.subtasks.map((step) => step.description)].some((text) => text.toLocaleLowerCase().includes(query)))
    return { category, tasks: matchingTasks, total: tasks.length, visible: (!props.archived || tasks.length > 0) && (matchesCategory || matchingTasks.length > 0) }
  }).filter((branch) => branch.visible)

  function setExpanded(id: number, expanded: boolean) {
    setCollapsedIds((previous) => {
      const next = new Set(previous)
      if (expanded) next.delete(id)
      else next.add(id)
      return next
    })
  }

  return (
    <Box component="section" aria-label="Categories and tasks">
      <Stack direction="row" sx={{ my: 1, justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap' }}>
        <Typography variant="caption" color="text.secondary">{query ? `${branches.length} matching ${branches.length === 1 ? 'category' : 'categories'}` : 'Categories / Tasks'}</Typography>
        <Stack direction="row" spacing={1}>
          <Button size="small" disabled={Boolean(query) || props.categories.length === 0 || collapsedIds.size === 0} onClick={() => setCollapsedIds(new Set())}>Expand all</Button>
          <Button size="small" disabled={Boolean(query) || props.categories.length === 0 || props.categories.every((category) => collapsedIds.has(category.id))} onClick={() => setCollapsedIds(new Set(props.categories.map((category) => category.id)))}>Collapse all</Button>
        </Stack>
      </Stack>
      <List disablePadding aria-label="Categories" sx={{ bgcolor: 'background.paper', border: branches.length ? 1 : 0, borderColor: 'divider', borderRadius: 2 }}>
        {branches.map(({ category, tasks, total }) => (
          <CategoryBranch key={category.id} {...props} category={category} matchingTasks={tasks} total={total}
            expanded={Boolean(query) || !collapsedIds.has(category.id)} searching={Boolean(query)}
            setExpanded={(expanded) => setExpanded(category.id, expanded)} />
        ))}
      </List>
      {query && branches.length === 0 && <Box sx={{ textAlign: 'center', py: 5 }}><Typography variant="h2" gutterBottom>No matches</Typography><Typography variant="body2" color="text.secondary">Try another category name or task description.</Typography></Box>}
    </Box>
  )
}

function CategoryBranch({ category, categories, matchingTasks, total, expanded, searching, setExpanded, disabled, archived,
  onUpdateCategory, onDeleteCategory, onCreateTask, onUpdateTask, onDeleteTask, onReplaceSubtasks, onSetCompleted }: Props & {
  category: Category
  matchingTasks: Task[]
  total: number
  expanded: boolean
  searching: boolean
  setExpanded: (expanded: boolean) => void
}) {
  const [editing, setEditing] = useState(false)
  const [creatingTask, setCreatingTask] = useState(false)
  const [menuAnchor, setMenuAnchor] = useState<HTMLElement | null>(null)
  const childrenId = `category-${category.id}-tasks`
  return (
    <ListItem disablePadding sx={{ display: 'block', '& + li': { borderTop: 1, borderColor: 'divider' } }}>
      <Stack direction="row" spacing={0.5} sx={{ py: 0.5, pl: 0.5, pr: 1, alignItems: 'center' }}>
        <Typography component="h2" sx={{ flex: 1, minWidth: 0 }}>
          <ButtonBase sx={{ width: '100%', minHeight: 48, p: 1, gap: 1, justifyContent: 'flex-start', textAlign: 'left', borderRadius: 1, '&:hover': { bgcolor: 'action.hover' } }} aria-label={`${category.description}, ${total} ${total === 1 ? 'task' : 'tasks'}`} aria-expanded={expanded}
            aria-controls={childrenId} disabled={searching} onClick={() => setExpanded(!expanded)}>
            {expanded ? <ExpandMoreIcon color="action" fontSize="small" /> : <ChevronRightIcon color="action" fontSize="small" />}
            <FolderOutlinedIcon color="primary" fontSize="small" sx={{ display: { xs: 'none', sm: 'block' } }} />
            <Box component="span" sx={{ fontWeight: 600, fontSize: '0.9rem', overflowWrap: 'anywhere', minWidth: 0 }}>{category.description}</Box>
            <Chip size="small" variant="outlined" label={searching && matchingTasks.length !== total ? `${matchingTasks.length}/${total}` : total} sx={{ fontSize: '0.7rem', height: 24 }} />
          </ButtonBase>
        </Typography>
        {!archived && <Stack direction="row" sx={{ flexShrink: 0, alignItems: 'center' }}>
          <IconButton color="primary" disabled={disabled} aria-label={`Add task to ${category.description}`} onClick={() => {
            setExpanded(true)
            setCreatingTask(true)
          }}><AddIcon fontSize="small" /></IconButton>
          <IconButton aria-label={`Actions for category ${category.description}`} aria-haspopup="menu" aria-expanded={menuAnchor ? true : undefined}
            aria-controls={menuAnchor ? `category-${category.id}-menu` : undefined} onClick={(event) => setMenuAnchor(event.currentTarget)}><MoreHorizIcon fontSize="small" /></IconButton>
          <Menu id={`category-${category.id}-menu`} anchorEl={menuAnchor} open={Boolean(menuAnchor)} onClose={() => setMenuAnchor(null)}>
              <MenuItem disabled={disabled} onClick={() => {
                setMenuAnchor(null)
                setEditing(true)
              }}><ListItemIcon><EditOutlinedIcon fontSize="small" /></ListItemIcon><ListItemText>Rename category</ListItemText></MenuItem>
              <MenuItem disabled={disabled} sx={{ color: 'error.main' }} onClick={() => {
                setMenuAnchor(null)
                void onDeleteCategory(category.id)
              }}><ListItemIcon sx={{ color: 'inherit' }}><DeleteOutlineIcon fontSize="small" /></ListItemIcon><ListItemText>Delete category</ListItemText></MenuItem>
          </Menu>
        </Stack>}
      </Stack>
      {!archived && editing && <Paper variant="outlined" sx={{ m: 2, p: 2 }}><CategoryForm initialDescription={category.description} disabled={disabled}
        onCancel={() => setEditing(false)} onSave={async (description) => {
          const saved = await onUpdateCategory(category.id, description)
          if (saved) setEditing(false)
          return saved
        }} /></Paper>}
      <Box id={childrenId} hidden={!expanded} sx={{ display: expanded ? 'block' : 'none', borderLeft: 1, borderColor: 'divider', ml: { xs: 2.5, sm: 3.5 }, mr: { xs: 1, sm: 2 }, pl: 1, mb: 2 }}>
        <List disablePadding aria-label={`Tasks in ${category.description}`}>
          {matchingTasks.map((task) => <TaskCard key={task.id} task={task} categories={categories} disabled={disabled} onUpdate={onUpdateTask} onDelete={onDeleteTask} onReplaceSubtasks={onReplaceSubtasks} onSetCompleted={onSetCompleted} />)}
        </List>
        {matchingTasks.length === 0 && <Typography variant="body2" color="text.secondary" sx={{ p: 1.5 }}>No tasks in this category.</Typography>}
        {!archived && creatingTask && <Paper variant="outlined" component="section" sx={{ p: 2, mt: 1 }} aria-label={`New task in ${category.description}`}>
          <Typography variant="h3" sx={{ mb: 2 }}>New task</Typography>
          <TaskForm categories={categories} preferredCategoryId={category.id} disabled={disabled} submitLabel="Create task"
            onCancel={() => setCreatingTask(false)} onSave={async (input) => {
              const saved = await onCreateTask(input)
              if (saved) setCreatingTask(false)
              return saved
            }} />
        </Paper>}
      </Box>
    </ListItem>
  )
}
