// Keep SDK access isolated. The shell can render outside Telegram, but private
// API access requires signed Mini App initData.
declare global {
  interface Window {
    Telegram?: {
      WebApp?: {
        initData?: string
        ready: () => void
        expand: () => void
        onEvent?: (event: 'themeChanged', handler: () => void) => void
        offEvent?: (event: 'themeChanged', handler: () => void) => void
      }
    }
  }
}

export function getTelegramInitData() {
  return window.Telegram?.WebApp?.initData
}

export function initializeTelegram() {
  window.Telegram?.WebApp?.ready()
  window.Telegram?.WebApp?.expand()
}

export function subscribeTelegramTheme(handler: () => void) {
  const app = window.Telegram?.WebApp
  app?.onEvent?.('themeChanged', handler)
  return () => app?.offEvent?.('themeChanged', handler)
}
