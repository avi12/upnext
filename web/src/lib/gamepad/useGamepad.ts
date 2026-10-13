import { useEffect, useRef } from 'react'
import { whenFocused } from './focus'
import { PadReader, type PadAction, type PadState } from './pad'

/** The input that came last, so a text field knows whether to open the
 *  on-screen keyboard (a pad) or leave typing to the system (keys, touch). */
export type InputKind = 'gamepad' | 'keyboard' | 'pointer'
let last: InputKind = 'pointer'

export function lastInput(): InputKind {
  return last
}

export function noteInput(kind: InputKind) {
  last = kind
}

function pads(): ReadonlyArray<PadState | null> {
  try {
    return typeof navigator.getGamepads === 'function' ? navigator.getGamepads() : []
  } catch {
    // A page in a frame without the gamepad permission throws.
    return []
  }
}

/** Calls `onAction` for each gamepad action, with the pad that sent it.
 *  The pads are read once per animation frame, and only while one is
 *  connected and the page is showing, and in the app window only once it
 *  has had the focus. Browsers hand a page its pads only after a button is
 *  pressed while it has the focus, so the first press may just wake it. */
export function useGamepad(onAction: (action: PadAction, pad: PadState) => void, enabled = true) {
  const latest = useRef(onAction)
  useEffect(() => {
    latest.current = onAction
  })

  useEffect(() => {
    if (!enabled) return
    const onKey = () => {
      last = 'keyboard'
    }
    const onPointer = () => {
      last = 'pointer'
    }
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('pointerdown', onPointer, true)
    // In the app window, only once it has had the focus (`whenFocused`).
    const stopPads = whenFocused(() => {
      const reader = new PadReader()
      let frame = 0
      const tick = (now: number) => {
        frame = 0
        const list = pads()
        if (!list.some((p) => p && p.connected !== false) || document.visibilityState === 'hidden') {
          reader.reset()
          return
        }
        const { actions, from } = reader.poll(list, now)
        if (from) {
          last = 'gamepad'
          for (const a of actions) latest.current(a, from)
        }
        frame = requestAnimationFrame(tick)
      }
      const start = () => {
        if (!frame && document.visibilityState !== 'hidden') frame = requestAnimationFrame(tick)
      }
      // A pad already connected before this page loaded shows up straight away.
      start()
      window.addEventListener('gamepadconnected', start)
      document.addEventListener('visibilitychange', start)
      return () => {
        if (frame) cancelAnimationFrame(frame)
        window.removeEventListener('gamepadconnected', start)
        document.removeEventListener('visibilitychange', start)
      }
    })
    return () => {
      stopPads()
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('pointerdown', onPointer, true)
    }
  }, [enabled])
}
