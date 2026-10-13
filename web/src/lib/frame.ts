// The desktop app's own title bar (the app's top bar,
// or a bar of its own): the window has no system title bar or frame on Windows and
// Linux, so the UI draws the window's buttons and asks the window, through
// `window.ipc`, to move, resize, minimize, maximize and close (`WindowAction`
// in src/desktop.rs). In a browser none of this exists.

declare global {
  interface Window {
    /** Set by the app's window (`APP_SCRIPT` in src/desktop.rs). */
    upnextApp?: boolean
  }
}

/** This page is in the UpNext app's own window, not a browser, whichever
 *  server the window shows (its own, the service or `upnext serve`). */
export function inApp() {
  return typeof window !== 'undefined' && window.upnextApp === true
}

export type FrameOs = 'windows' | 'linux'

/** A resize edge or corner, by compass point (`window:resize:<edge>`). */
export type Edge = 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w' | 'nw'

/** What the title bar asks of the window. */
export type WindowAction = 'framed' | 'drag' | 'minimize' | 'maximize' | 'close' | `resize:${Edge}`

/** How the window is, from the `upnext:window` event. */
export interface WindowState {
  maximized: boolean
  focused: boolean
  /** Full screen (F11, or a window opened in big screen): no title bar.
   *  Missing from the window of an older UpNext. */
  fullscreen?: boolean
}

declare global {
  interface Window {
    /** Set by the frameless app window (`FRAME_SCRIPT` in src/desktop.rs). */
    upnextFrame?: { os?: string }
    /** wry's channel to the window. */
    ipc?: { postMessage(message: string): void }
  }
}

/** The system whose frameless window this page is in, or null in a browser
 *  (and in an app window with the system's title bar). Read from what the
 *  window injects, never from the user agent. */
export function frameOs(): FrameOs | null {
  if (typeof window === 'undefined') return null
  const os = window.upnextFrame?.os
  return os === 'windows' || os === 'linux' ? os : null
}

/** Ask the window for something. */
export function windowAction(action: WindowAction) {
  window.ipc?.postMessage(`window:${action}`)
}

/** The edges the UI resizes the window from. Windows keeps its own resize
 *  borders on the left, right and bottom, outside the window, with the
 *  shadow, so only the top is left to the UI; on Linux the window has no
 *  frame at all, so the UI makes all four sides and the corners. */
export function resizeEdges(os: FrameOs): Edge[] {
  return os === 'windows' ? ['n', 'ne', 'nw'] : ['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw']
}

/** Mark the page as being in the frameless window before its first render
 *  (index.css). */
export function markFrame() {
  const os = frameOs()
  if (os) document.documentElement.dataset.frame = os
}

/** The slim title bar's height in pixels: 0 unless the frameless window
 *  shows it, on a page with no top bar. Parts of the UI placed by hand
 *  against the top of the window keep clear of it. */
export function titleBarHeight(): number {
  if (typeof document === 'undefined') return 0
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--titlebar-h')) || 0
}

/** The page scrollbar's thumb in the Windows app window (PageScrollbar.tsx). */
export interface Thumb {
  top: number
  height: number
}

/** The smallest the thumb goes, so it can still be grabbed on a long page. */
const MIN_THUMB = 32

/** Where the thumb goes for the page as it's scrolled now, between the
 *  title bar (`ceiling`) and the bottom of the window; null when the page
 *  doesn't scroll. */
export function thumbFor(scrollTop: number, scrollHeight: number, viewHeight: number, ceiling: number): Thumb | null {
  const scrollable = scrollHeight - viewHeight
  if (scrollable <= 1) return null
  const track = viewHeight - ceiling
  const height = Math.max(MIN_THUMB, Math.round((track * viewHeight) / scrollHeight))
  const top = ceiling + Math.round(((track - height) * Math.min(Math.max(scrollTop, 0), scrollable)) / scrollable)
  return { top, height }
}
