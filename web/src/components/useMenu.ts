import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react'

const ITEMS = '[role^="menuitem"]:not([disabled])'

/** Shared behaviour for every dropdown menu: focus the checked (or first)
 *  item on open, close on a click outside or Esc/Tab (focus back to the
 *  button), and move with ↑/↓/Home/End. ↓ on the button opens it. */
export function useMenu<W extends HTMLElement = HTMLElement>() {
  const [open, setOpen] = useState(false)
  const wrapRef = useRef<W>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const id = useId()

  const items = () => Array.from(menuRef.current?.querySelectorAll<HTMLElement>(ITEMS) ?? [])

  useEffect(() => {
    if (!open) return
    const all = items()
    ;(all.find((b) => b.getAttribute('aria-checked') === 'true') ?? all[0])?.focus()
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [open])

  const close = () => {
    setOpen(false)
    buttonRef.current?.focus()
  }

  const onMenuKeyDown = (e: KeyboardEvent) => {
    const all = items()
    const at = all.indexOf(document.activeElement as HTMLElement)
    const go = (i: number) => {
      e.preventDefault()
      all[(i + all.length) % all.length]?.focus()
    }
    if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault()
      // Don't let Esc also close the sheet or dialog underneath.
      e.stopPropagation()
      e.nativeEvent.stopImmediatePropagation()
      close()
    } else if (e.key === 'ArrowDown') go(at + 1)
    else if (e.key === 'ArrowUp') go(at - 1)
    else if (e.key === 'Home') go(0)
    else if (e.key === 'End') go(all.length - 1)
  }

  const buttonProps = {
    ref: buttonRef,
    type: 'button' as const,
    'aria-haspopup': 'menu' as const,
    'aria-expanded': open,
    'aria-controls': open ? id : undefined,
    onClick: () => setOpen((o) => !o),
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown' && !open) {
        e.preventDefault()
        setOpen(true)
      }
    },
  }
  const menuProps = { ref: menuRef, id, role: 'menu' as const, onKeyDown: onMenuKeyDown }

  return { open, setOpen, close, wrapRef, buttonProps, menuProps }
}
