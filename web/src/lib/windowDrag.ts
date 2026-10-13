import type { MouseEvent as ReactMouseEvent } from 'react'
import { frameOs, windowAction } from './frame'

/** How far the pointer moves with the button down on a bar before the
 *  window moves with it, so a double click still reaches the page. */
const DRAG_AFTER = 4

/** What's pressed on a bar that works itself rather than drags the window. */
const CONTROLS = 'a, button, input, textarea, select, label, summary, [role], [tabindex], [contenteditable], [data-no-drag]'

/** Where the pointer went down on a bar, until it moves far enough to drag. */
let down: { x: number; y: number } | null = null

const onBar = (e: ReactMouseEvent) => e.button === 0 && !(e.target as HTMLElement).closest(CONTROLS)

/** The handlers that make a bar move the window (a page's own slim
 *  title bar, and the top bars). */
export const windowDrag = {
  onMouseDown(e: ReactMouseEvent) {
    if (!onBar(e)) return
    // No text selection or focus change from the bar.
    e.preventDefault()
    down = { x: e.clientX, y: e.clientY }
  },
  onDoubleClick(e: ReactMouseEvent) {
    if (onBar(e)) windowAction('maximize')
  },
}

/** What makes a top bar move the window: pressing anywhere on it but its
 *  links, buttons and fields and moving, or a double click to maximize or
 *  restore. Nothing in a browser. */
export function useWindowDrag(): Partial<typeof windowDrag> {
  return frameOs() ? windowDrag : {}
}

/** Move the window once the pointer has moved a little with the button down
 *  on a bar, wherever it has got to by then. Returns the cleanup. */
export function trackDrag() {
  const move = (e: MouseEvent) => {
    if (!down) return
    if ((e.buttons & 1) === 0) {
      down = null
    } else if (Math.abs(e.clientX - down.x) + Math.abs(e.clientY - down.y) >= DRAG_AFTER) {
      down = null
      windowAction('drag')
    }
  }
  const up = () => {
    down = null
  }
  window.addEventListener('mousemove', move)
  window.addEventListener('mouseup', up)
  return () => {
    window.removeEventListener('mousemove', move)
    window.removeEventListener('mouseup', up)
  }
}
