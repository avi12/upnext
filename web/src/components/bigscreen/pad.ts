import { createContext, useContext, useEffect, useRef, type RefObject } from 'react'
import type { PadAction, PadStyle } from '../../lib/gamepad/pad'

/** Whose button names big screen shows: the last pad used (or the one picked
 *  in Big screen options), or null for keys when no pad has been used. */
export const PadStyleContext = createContext<PadStyle | null>(null)

export function usePadStyle(): PadStyle | null {
  return useContext(PadStyleContext)
}

/** The event BigShell sends to the focused element for every action, before
 *  doing its own thing: a card takes X for its menu, the on-screen keyboard
 *  takes X and Y for delete and space. Calling preventDefault() keeps
 *  BigShell from acting on it too. */
export const ACTION_EVENT = 'bigscreen:action'

export type ActionEvent = CustomEvent<PadAction>

/** Sends `action` to the focused element; true when something took it. */
export function offerAction(action: PadAction): boolean {
  const ev = new CustomEvent<PadAction>(ACTION_EVENT, { detail: action, bubbles: true, cancelable: true })
  const target = document.activeElement && document.activeElement !== document.body ? document.activeElement : null
  target?.dispatchEvent(ev)
  return ev.defaultPrevented
}

/** Calls `handle` for actions sent to anything inside `ref`; it returns true
 *  for an action it took. */
export function useActions<T extends HTMLElement>(handle: (action: PadAction) => boolean): RefObject<T | null> {
  const ref = useRef<T>(null)
  const latest = useRef(handle)
  useEffect(() => {
    latest.current = handle
  })
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const on = (e: Event) => {
      if (latest.current((e as ActionEvent).detail)) e.preventDefault()
    }
    el.addEventListener(ACTION_EVENT, on)
    return () => el.removeEventListener(ACTION_EVENT, on)
  }, [])
  return ref
}
