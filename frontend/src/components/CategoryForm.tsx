import { useState } from 'react'
import { Box, Button, Stack, TextField } from '@mui/material'

export function CategoryForm({ initialDescription = '', disabled, onSave, onCancel }: {
  initialDescription?: string
  disabled: boolean
  onSave: (description: string) => Promise<boolean>
  onCancel: () => void
}) {
  const [description, setDescription] = useState(initialDescription)
  return (
    <Box component="form" onSubmit={async (event) => {
      event.preventDefault()
      if (description.trim()) await onSave(description.trim())
    }}>
      <Stack spacing={2}>
        <TextField label="Category description" autoFocus placeholder="e.g. Work, Home, Ideas" value={description}
          onChange={(event) => setDescription(event.target.value)} required disabled={disabled} />
        <Stack direction="row" spacing={1}>
          <Button type="submit" variant="contained" disabled={disabled || !description.trim()}>{initialDescription ? 'Save category' : 'Create category'}</Button>
          <Button disabled={disabled} onClick={onCancel}>Cancel</Button>
        </Stack>
      </Stack>
    </Box>
  )
}
