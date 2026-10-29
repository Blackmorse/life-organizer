import { useEffect, useState } from 'react'
import type { PropsWithChildren } from 'react'
import { CssBaseline, ThemeProvider, createTheme, useMediaQuery } from '@mui/material'
import { subscribeTelegramTheme } from './telegram'

export function AppTheme({ children }: PropsWithChildren) {
  const prefersDark = useMediaQuery('(prefers-color-scheme: dark)')
  const [, setRevision] = useState(0)
  useEffect(() => subscribeTelegramTheme(() => setRevision((value) => value + 1)), [])
  // This provider rerenders on OS/Telegram theme changes. Read SDK CSS colors
  // again on each render rather than caching colors that live outside React.
  const css = getComputedStyle(document.documentElement)
  const color = (name: string, fallback: string) => css.getPropertyValue(`--tg-theme-${name}`).trim() || fallback
  const scheme = css.getPropertyValue('--tg-color-scheme').trim()
  const dark = scheme ? scheme === 'dark' : prefersDark
  const theme = createTheme({
    palette: {
      mode: dark ? 'dark' : 'light',
      primary: {
        main: color('button-color', dark ? '#8fcab0' : '#306850'),
        contrastText: color('button-text-color', dark ? '#15251d' : '#ffffff'),
      },
      background: {
        default: color('bg-color', dark ? '#171d1b' : '#f5f5ef'),
        paper: color('secondary-bg-color', dark ? '#222b27' : '#fffef9'),
      },
      text: {
        primary: color('text-color', dark ? '#e5eee8' : '#253b36'),
        secondary: color('hint-color', dark ? '#a2b1a8' : '#6d7972'),
      },
      error: { main: color('destructive-text-color', dark ? '#f09c91' : '#b45344') },
    },
    typography: {
      fontFamily: 'Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      h1: { fontSize: '1.8rem', fontWeight: 600, letterSpacing: '-0.04em' },
      h2: { fontSize: '1rem', fontWeight: 600 },
      h3: { fontSize: '1rem', fontWeight: 600 },
      button: { textTransform: 'none', fontWeight: 600 },
    },
    shape: { borderRadius: 12 },
    components: {
      MuiButton: { defaultProps: { disableElevation: true }, styleOverrides: { root: { minHeight: 44 } } },
      MuiIconButton: { styleOverrides: { root: { minWidth: 44, minHeight: 44 } } },
      MuiTextField: { defaultProps: { fullWidth: true, size: 'small' } },
    },
  })

  return <ThemeProvider theme={theme}><CssBaseline />{children}</ThemeProvider>
}
