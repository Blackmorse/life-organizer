import { useState } from 'react'
import { Box, Button, Collapse, Typography } from '@mui/material'

export function DetailedDescription({ text }: { text?: string }) {
  return text ? <Typography variant="body2" color="text.secondary" sx={{ mt: 1, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{text}</Typography> : null
}

export function DetailedDescriptionDisclosure({ text, label }: { text?: string, label: string }) {
  const [expanded, setExpanded] = useState(false)
  if (!text) return null
  return <Box>
    <Button size="small" aria-label={`View details for ${label}`} aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? 'Hide details' : 'Show details'}</Button>
    <Collapse in={expanded} unmountOnExit><DetailedDescription text={text} /></Collapse>
  </Box>
}
