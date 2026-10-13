import { nearest, type Box, type Dir } from './geometry'

/** What can take the focus: the elements a keyboard reaches with Tab, plus
 *  anything marked `data-focusable`. `data-focus-skip` leaves one out. */
const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
  '[data-focusable]',
].join(',')

/** Each row or grid (`data-focus-group`) remembers the item last focused in
 *  it, so coming back to it lands where the person left off. */
const memory = new WeakMap<Element, HTMLElement>()

/** Where the focus may go now: the newest open sheet, menu or keyboard
 *  (`data-focus-scope`), else the whole page. */
export function scopeRoot(doc: Document = document): HTMLElement {
  const scopes = doc.querySelectorAll<HTMLElement>('[data-focus-scope]')
  return scopes.length ? scopes[scopes.length - 1] : doc.body
}

function shown(el: HTMLElement): boolean {
  if (el.closest('[inert], [aria-hidden="true"], [data-focus-skip]')) return false
  const r = el.getBoundingClientRect()
  return r.width > 0 && r.height > 0
}

/** The elements in `root` that can take the focus, in page order. */
export function focusables(root: HTMLElement = scopeRoot()): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(shown)
}

function box(el: HTMLElement): Box {
  const r = el.getBoundingClientRect()
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

/** Focuses `el` and brings it into view, without the jump a plain focus()
 *  makes when the element is half off screen. */
export function focusElement(el: HTMLElement) {
  el.focus({ preventScroll: true })
  const reduce = typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
  el.scrollIntoView?.({ block: 'nearest', inline: 'nearest', behavior: reduce ? 'auto' : 'smooth' })
}

/** The item a group last had focused, if it can still take the focus. */
function remembered(group: Element, among: HTMLElement[]): HTMLElement | null {
  const el = memory.get(group)
  return el && el.isConnected && among.includes(el) ? el : null
}

/** The item a scope starts on: the one marked `data-focus-first`, else, on
 *  the page, the first item in its <main> (not the top bar), else the first
 *  focusable. */
export function firstItem(root: HTMLElement = scopeRoot()): HTMLElement | null {
  const all = focusables(root)
  const first =
    all.find((el) => el.hasAttribute('data-focus-first')) ?? (root === root.ownerDocument.body ? all.find((el) => el.closest('main')) : undefined) ?? all[0]
  if (!first) return null
  const group = first.closest('[data-focus-group]')
  return (group && remembered(group, all)) || first
}

/** Focuses the first thing in the scope (see firstItem). False when there's
 *  nothing to focus. */
export function focusFirst(root: HTMLElement = scopeRoot()): boolean {
  const first = firstItem(root)
  if (!first) return false
  focusElement(first)
  return true
}

/** Moves the focus one step in `dir` within the current scope. With nothing
 *  focused there yet, it focuses the first item instead. False when nothing
 *  lies that way, so the caller can leave the key to the page. */
export function move(dir: Dir): boolean {
  const root = scopeRoot()
  const all = focusables(root)
  const current = document.activeElement as HTMLElement | null
  if (!current || current === document.body || !root.contains(current) || !all.includes(current)) return focusFirst(root)
  const others = all.filter((el) => el !== current && !el.contains(current) && !current.contains(el))
  const i = nearest(box(current), others.map(box), dir)
  if (i < 0) return false
  let target = others[i]
  // Entering a row or grid from outside it: back to the item it had.
  const group = target.closest('[data-focus-group]')
  if (group && !group.contains(current)) target = remembered(group, all) ?? target
  focusElement(target)
  return true
}

/** Remembers, for every group around it, the element that just took the
 *  focus. Installed once by the big screen; returns its removal. */
export function trackGroups(doc: Document = document): () => void {
  const onFocus = (e: FocusEvent) => {
    const el = e.target as HTMLElement
    for (let g = el.parentElement?.closest('[data-focus-group]'); g; g = g.parentElement?.closest('[data-focus-group]')) {
      memory.set(g, el)
    }
  }
  doc.addEventListener('focusin', onFocus)
  return () => doc.removeEventListener('focusin', onFocus)
}
