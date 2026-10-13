import { useEffect } from 'react'
import type { ThemeName } from '../types'
import { useSettings } from './queries'

const THEME_COLOR: Record<ThemeName, string> = { arcade: '#101218', ember: '#151412', daylight: '#f7f3ec' }

function effective(theme: ThemeName, follow: boolean): ThemeName {
  if (follow && window.matchMedia('(prefers-color-scheme: light)').matches) return 'daylight'
  return theme
}

/** Set <html data-theme> and remember it for the pre-paint script in index.html. */
export function applyTheme(theme: ThemeName, follow: boolean) {
  const t = effective(theme, follow)
  document.documentElement.dataset.theme = t
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', THEME_COLOR[t])
  try {
    localStorage.setItem('upnext.theme', theme)
    localStorage.setItem('upnext.themeFollow', follow ? '1' : '0')
  } catch {
    /* private mode: the theme still applies for this session */
  }
}

/** Keep the page theme in step with Settings (and the OS, when following it). */
export function useThemeSync() {
  const settings = useSettings()
  const theme = settings.data?.theme ?? 'arcade'
  const follow = settings.data?.theme_follow_system ?? false
  useEffect(() => {
    if (!settings.data) return
    applyTheme(theme, follow)
    if (!follow) return
    const mq = window.matchMedia('(prefers-color-scheme: light)')
    const onChange = () => applyTheme(theme, follow)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [settings.data, theme, follow])
}
