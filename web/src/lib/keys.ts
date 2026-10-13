/** True while the person is typing in a field. */
export function typingInField(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null
  return !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable)
}

/** Page-level single-key shortcuts (1–5, /, R) stay quiet while typing, with
 *  a modifier held, or while a dialog or menu is open on top of the page. */
export function shortcutBlocked(e: KeyboardEvent): boolean {
  return (
    typingInField(e.target) ||
    e.metaKey ||
    e.ctrlKey ||
    e.altKey ||
    !!document.querySelector('[role="dialog"], [role="menu"], [role="listbox"]')
  )
}
