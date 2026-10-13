import { useEffect, type RefObject } from 'react'

/** What Tab moves between inside a trap. */
const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])'

/** The traps shown, oldest first. "Who's playing?" and the Allow dialog can
 *  both be up at once, and only one of them may move the focus on a Tab. */
const shown: RefObject<HTMLElement | null>[] = []

/** Tab and Shift+Tab stay inside `root` while it's shown: the page a modal
 *  dialog covers can't be reached until the dialog goes. Focus outside it
 *  comes in at the first (or, going back, the last) thing it holds. With
 *  two traps up, the one holding the focus keeps it; focus in neither goes
 *  to the newer. */
export function useTabTrap(root: RefObject<HTMLElement | null>) {
  useEffect(() => {
    shown.push(root)
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || !root.current) return
      const at = document.activeElement
      const holder = shown.find((r) => r.current?.contains(at)) ?? shown[shown.length - 1]
      if (holder !== root) return
      const items = [...root.current.querySelectorAll<HTMLElement>(FOCUSABLE)]
      if (!items.length) return
      const i = items.indexOf(at as HTMLElement)
      const to = i < 0 ? (e.shiftKey ? items.length - 1 : 0) : e.shiftKey ? (i === 0 ? items.length - 1 : -1) : i === items.length - 1 ? 0 : -1
      if (to < 0) return
      e.preventDefault()
      items[to].focus()
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      shown.splice(shown.indexOf(root), 1)
    }
  }, [root])
}
