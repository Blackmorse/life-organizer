import { useEffect, useState } from 'react'
import { Alert, AlertTitle, Avatar, Box, Button, Container, Divider, IconButton, InputAdornment, LinearProgress, Paper, Stack, Tab, Tabs, TextField, Typography } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import CloseIcon from '@mui/icons-material/Close'
import RefreshIcon from '@mui/icons-material/Refresh'
import SearchIcon from '@mui/icons-material/Search'
import { CategoryForm } from './components/CategoryForm'
import { CategoryTree } from './components/CategoryTree'
import { useWorkspace } from './hooks/useWorkspace'
import { initializeTelegram } from './telegram'

export default function App() {
  useEffect(initializeTelegram, [])
  const workspace = useWorkspace()
  const [query, setQuery] = useState('')
  const [creatingCategory, setCreatingCategory] = useState(false)
  const [tab, setTab] = useState<'active' | 'archive'>('active')
  const archived = tab === 'archive'
  const visibleTasks = workspace.tasks.filter((task) => task.completed === archived)
  const disabled = workspace.loading || workspace.saving

  if (workspace.authorizationRequired) return (
    <Container component="main" maxWidth="sm" sx={{ py: 'max(32px, var(--tg-content-safe-area-inset-top, 0px))' }}>
      <Typography variant="h1" sx={{ mb: 2 }}>Your personal Scheduler</Typography>
      <Alert severity="info">
        <AlertTitle>Telegram sign-in required</AlertTitle>
        {workspace.error}
      </Alert>
    </Container>
  )

  return (
    <Container maxWidth="md" sx={{
      pt: 'max(24px, var(--tg-content-safe-area-inset-top, 0px))',
      pb: 'max(24px, var(--tg-content-safe-area-inset-bottom, 0px))',
      pl: 'max(16px, var(--tg-content-safe-area-inset-left, 0px))',
      pr: 'max(16px, var(--tg-content-safe-area-inset-right, 0px))',
    }}>
      <Stack component="header" direction="row" spacing={2} sx={{ pb: 2, alignItems: 'center', justifyContent: 'space-between' }}>
        <Stack direction="row" spacing={1.5} sx={{ alignItems: 'center' }}>
          <Avatar variant="rounded" sx={{ bgcolor: 'primary.main', color: 'primary.contrastText', fontWeight: 600 }} aria-hidden="true">s.</Avatar>
          <Typography sx={{ fontWeight: 600 }}>scheduler<Box component="span" sx={{ display: { xs: 'none', sm: 'inline' }, color: 'text.secondary', fontSize: '0.8rem', fontWeight: 400 }}> / personal workspace</Box></Typography>
        </Stack>
        <Button startIcon={<RefreshIcon />} disabled={disabled} onClick={workspace.refresh}>Refresh</Button>
      </Stack>
      <Divider />
      <Box component="main">
        <Stack direction="row" spacing={2} sx={{ mt: 3, mb: 1, justifyContent: 'space-between', alignItems: 'center' }}>
          <Typography variant="h1">{archived ? 'Archive' : 'Your workspace'}</Typography>
          {!archived && <Button variant="outlined" startIcon={<AddIcon />} disabled={disabled || creatingCategory} onClick={() => setCreatingCategory(true)}>Category</Button>}
        </Stack>
        <Tabs aria-label="Task views" value={tab} onChange={(_, value: 'active' | 'archive') => {
          setTab(value)
          setQuery('')
        }} sx={{ mb: 2 }}>
          <Tab value="active" label="Active" id="active-tab" aria-controls="active-panel" />
          <Tab value="archive" label="Archive" id="archive-tab" aria-controls="archive-panel" />
        </Tabs>
        {archived && <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>Completed tasks are read-only. Restore a task to edit it or move it back to your active workspace.</Typography>}
        <Typography component="div" variant="body2" color="text.secondary" role="status" aria-live="polite" sx={{ mb: 2 }}>
          {workspace.loading ? 'Loading workspace…' : workspace.saving ? 'Saving changes…' : `${archived ? 'Archived' : 'Active'} · ${visibleTasks.length} ${visibleTasks.length === 1 ? 'task' : 'tasks'}`}
        </Typography>
        {disabled && <LinearProgress aria-label={workspace.loading ? 'Loading workspace' : 'Saving changes'} sx={{ mb: 2 }} />}

        {workspace.error && <Alert severity="error" sx={{ mb: 2 }}>
          <AlertTitle>Could not complete request</AlertTitle>
          {workspace.error}
          <Box><Button color="inherit" disabled={disabled} onClick={workspace.refresh}>Retry loading data</Button></Box>
        </Alert>}

        <TextField type="search" label="Search categories and tasks" value={query} onChange={(event) => setQuery(event.target.value)} sx={{ mt: 1 }}
          slotProps={{ input: {
            startAdornment: <InputAdornment position="start"><SearchIcon /></InputAdornment>,
            endAdornment: query ? <InputAdornment position="end"><IconButton aria-label="Clear search" size="small" onClick={() => setQuery('')}><CloseIcon /></IconButton></InputAdornment> : undefined,
          } }} />

        {!archived && creatingCategory && <Paper variant="outlined" component="section" aria-labelledby="new-category-title" sx={{ p: 2.5, mt: 2 }}>
          <Typography variant="h2" id="new-category-title" sx={{ mb: 2 }}>New category</Typography>
          <CategoryForm disabled={disabled} onCancel={() => setCreatingCategory(false)} onSave={async (description) => {
            const saved = await workspace.createCategory(description)
            if (saved) {
              setCreatingCategory(false)
              setQuery('')
            }
            return saved
          }} />
        </Paper>}

        <Box role="tabpanel" id={`${tab}-panel`} aria-labelledby={`${tab}-tab`} aria-busy={disabled}>
          <CategoryTree key={tab} categories={workspace.categories} tasks={visibleTasks} query={query} disabled={disabled} archived={archived}
            onUpdateCategory={workspace.updateCategory} onDeleteCategory={workspace.deleteCategory}
            onCreateTask={async (input) => {
              const saved = await workspace.createTask(input)
              if (saved) setQuery('')
              return saved
             }} onUpdateTask={workspace.updateTask} onDeleteTask={workspace.deleteTask} onReplaceSubtasks={workspace.replaceSubtasks} onSetCompleted={workspace.setCompleted} />
          {!workspace.loading && !workspace.error && (archived ? visibleTasks.length === 0 : workspace.categories.length === 0) && !query && <Box sx={{ textAlign: 'center', py: 5 }}>
            <Typography variant="h2" gutterBottom>{archived ? 'Your archive is empty' : 'Your workspace is empty'}</Typography>
            <Typography variant="body2" color="text.secondary">{archived ? 'Completed tasks will appear here.' : 'Create a category to give your tasks a home.'}</Typography>
          </Box>}
        </Box>
      </Box>
      <Box component="footer" sx={{ borderTop: 1, borderColor: 'divider', mt: 4, pt: 2 }}>
        <Typography variant="caption" color="text.secondary">Explore first. One task at a time.</Typography>
      </Box>
    </Container>
  )
}
