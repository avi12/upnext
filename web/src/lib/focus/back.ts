import { useEffect, useRef } from 'react'

/** What Back (Escape, Backspace, or B on a pad) closes, newest first: an open
 *  keyboard, menu or sheet registers here while it's open, so Back closes it
 *  before it leaves the page. */
const stack: Array<{ close: () => void }> = []

/** Registers `close` as the first thing Back does; returns its removal. */
export function pushBack(close: () => void): () => void {
  const entry = { close }
  stack.push(entry)
  return () => {
    const i = stack.indexOf(entry)
    if (i >= 0) stack.splice(i, 1)
  }
}

/** Runs the newest registered close. False when nothing is open, so the
 *  caller goes back a page instead. */
export function closeTop(): boolean {
  const top = stack[stack.length - 1]
  if (!top) return false
  top.close()
  return true
}

/** While `open`, Back calls `close` before anything opened earlier. */
export function useBack(open: boolean, close: () => void) {
  const latest = useRef(close)
  useEffect(() => {
    latest.current = close
  })
  useEffect(() => {
    if (!open) return
    return pushBack(() => latest.current())
  }, [open])
}
