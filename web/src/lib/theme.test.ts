import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { applyTheme } from './theme'

const prefersLight = (light: boolean) =>
  vi.spyOn(window, 'matchMedia').mockImplementation(
    (q: string) => ({ matches: light && q.includes('light'), media: q }) as MediaQueryList,
  )

describe('theme', () => {
  beforeEach(() => {
    // jsdom has no matchMedia of its own.
    window.matchMedia ??= (() => ({ matches: false })) as unknown as typeof window.matchMedia
    document.head.innerHTML = '<meta name="theme-color" content="">'
  })
  afterEach(() => {
    vi.restoreAllMocks()
    localStorage.clear()
  })

  it('sets the theme, its colour, and remembers both settings', () => {
    prefersLight(true)
    applyTheme('ember', false)
    expect(document.documentElement.dataset.theme).toBe('ember')
    expect(document.querySelector('meta[name="theme-color"]')?.getAttribute('content')).toBe('#151412')
    expect(localStorage.getItem('upnext.theme')).toBe('ember')
    expect(localStorage.getItem('upnext.themeFollow')).toBe('0')
  })

  it('follows a light system theme with daylight, but remembers the choice', () => {
    prefersLight(true)
    applyTheme('arcade', true)
    expect(document.documentElement.dataset.theme).toBe('daylight')
    expect(localStorage.getItem('upnext.theme')).toBe('arcade')
    expect(localStorage.getItem('upnext.themeFollow')).toBe('1')

    prefersLight(false)
    applyTheme('arcade', true)
    expect(document.documentElement.dataset.theme).toBe('arcade')
  })
})
