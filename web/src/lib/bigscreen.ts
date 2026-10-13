import { useSyncExternalStore } from 'react'
import type { PadStyle } from './gamepad/pad'

/** Big screen: the same UI with large type and one focus that a
 *  gamepad, arrow keys or a TV remote moves. It is remembered per browser,
 *  and `?big=1` (or `?big=0`) in the address turns it on (or off). An app
 *  window opened with `--big-screen` starts in it whatever is
 *  remembered, and leaves what's remembered for the windows opened without
 *  it: a switch made in that window lasts as long as the window. */
const KEY = 'upnext.bigscreen'
/** A switch made in a window opened in big screen, kept for that window. */
const WINDOW_KEY = 'upnext.bigscreen.window'
const OPTIONS_KEY = 'upnext.bigscreen.options'
/** What the app window was last told, kept for the window's life. */
const TOLD_KEY = 'upnext.bigscreen.told'
/** Set once UpNext has offered big screen after a gamepad press. */
export const OFFERED_KEY = 'upnext.bigscreen.offered'

export interface BigScreenOptions {
  /** Whose button names to show: the pad's own (auto) or one picked. */
  buttons: 'auto' | PadStyle
  /** A margin around everything, for a TV that cuts off its edges. */
  overscan: boolean
}

const DEFAULTS: BigScreenOptions = { buttons: 'auto', overscan: false }

function read(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* private mode: it still holds until the page reloads */
  }
}

declare global {
  interface Window {
    /** Set by an app window opened in big screen (`BIG_SCRIPT` in src/desktop.rs). */
    upnextBigScreen?: boolean
  }
}

/** Whether this page is in an app window opened in big screen. */
export function openedInBigScreen(): boolean {
  return typeof window !== 'undefined' && window.upnextBigScreen === true
}

function readWindow(): string | null {
  try {
    return sessionStorage.getItem(WINDOW_KEY)
  } catch {
    return null
  }
}

function writeWindow(value: string) {
  try {
    sessionStorage.setItem(WINDOW_KEY, value)
  } catch {
    /* it still holds until the page reloads */
  }
}

function readOptions(): BigScreenOptions {
  try {
    return { ...DEFAULTS, ...(JSON.parse(read(OPTIONS_KEY) ?? '{}') as Partial<BigScreenOptions>) }
  } catch {
    return DEFAULTS
  }
}

/** What `?big=` asks for: true, false, or null when it isn't there. */
export function askedInAddress(search: string): boolean | null {
  const v = new URLSearchParams(search).get('big')
  if (v === null) return null
  return v !== '0' && v !== 'false' && v !== 'off'
}

let on = openedInBigScreen() ? readWindow() !== '0' : read(KEY) === '1'
let options = readOptions()
const listeners = new Set<() => void>()

{
  const asked = typeof window === 'undefined' ? null : askedInAddress(window.location.search)
  if (asked !== null) {
    on = asked
    if (openedInBigScreen()) writeWindow(asked ? '1' : '0')
    else write(KEY, asked ? '1' : '0')
  }
}

function changed() {
  for (const l of listeners) l()
}

function subscribe(l: () => void) {
  listeners.add(l)
  return () => listeners.delete(l)
}

export function isBigScreen(): boolean {
  return on
}

export function setBigScreen(value: boolean) {
  if (value === on) return
  on = value
  if (openedInBigScreen()) writeWindow(value ? '1' : '0')
  else write(KEY, value ? '1' : '0')
  changed()
}

export function setBigScreenOptions(patch: Partial<BigScreenOptions>) {
  options = { ...options, ...patch }
  write(OPTIONS_KEY, JSON.stringify(options))
  changed()
}

/** Tell the app window when big screen goes on or off, so it fills the
 *  screen with it and gives back the windowed size after (`big-screen:` in
 *  src/desktop.rs). Only a change is told: a reload in big screen would
 *  otherwise undo leaving full screen with F11. Nothing in a browser. */
export function tellWindow(value: boolean) {
  if (typeof window === 'undefined' || window.upnextApp !== true) return
  const message = `big-screen:${value ? 'on' : 'off'}`
  let told: string | null = null
  try {
    told = sessionStorage.getItem(TOLD_KEY)
    sessionStorage.setItem(TOLD_KEY, message)
  } catch {
    /* without it, every load tells the window, as a change would */
  }
  if (told !== message) window.ipc?.postMessage(message)
}

/** Whether big screen is on, kept in step with every switch. */
export function useBigScreen(): boolean {
  return useSyncExternalStore(subscribe, () => on)
}

export function useBigScreenOptions(): BigScreenOptions {
  return useSyncExternalStore(subscribe, () => options)
}
