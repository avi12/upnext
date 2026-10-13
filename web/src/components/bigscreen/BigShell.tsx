import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { NavLink, Outlet, useLocation, useNavigate } from 'react-router'
import { firstItem, focusElement, focusFirst, focusables, move, scopeRoot, trackGroups } from '../../lib/focus/engine'
import { closeTop } from '../../lib/focus/back'
import type { Dir } from '../../lib/focus/geometry'
import { padStyle, type PadAction, type PadStyle } from '../../lib/gamepad/pad'
import { useGamepad } from '../../lib/gamepad/useGamepad'
import { setBigScreen, useBigScreenOptions } from '../../lib/bigscreen'
import { inApp, windowAction } from '../../lib/frame'
import { typingInField } from '../../lib/keys'
import { useThemeSync } from '../../lib/theme'
import { cx } from '../../lib/format'
import { PlayMark } from '../Shell'
import { BigSheet } from './BigMenu'
import { Glyph, Hint } from './Glyph'
import { PadStyleContext, offerAction } from './pad'
import { BigTour } from './BigTour'
import { BigPlayerButton } from './BigPlayers'
import { WindowControls } from '../WindowFrame'
import { useWindowDrag } from '../../lib/windowDrag'
import { usePlayerSwitches } from '../../lib/players'

/** Big screen's pages, in the order the bumpers step through them. */
const BIG_TABS = [
  { to: '/', label: 'Curate', end: true },
  { to: '/library', label: 'Library' },
  { to: '/lists', label: 'Lists' },
  { to: '/versus', label: 'Help me decide' },
  { to: '/settings', label: 'Settings' },
]

/** Big screen's focus ring, in rem: its width and its gap from what it
 *  rings. index.css draws it from the two properties BigShell sets. */
const RING = { width: 0.2, offset: 0.2 }
/** Room around the tabs for the ring, and a little more. */
const RING_ROOM = `${Math.round((RING.width + RING.offset + 0.05) * 100) / 100}rem`

/** The keys that stand for pad buttons, for a keyboard, a remote or Steam
 *  Input's keyboard template. Arrows, Enter and Escape are handled apart. */
const KEY_ACTIONS: Record<string, PadAction> = {
  '[': 'prevTab',
  ']': 'nextTab',
  PageUp: 'pageUp',
  PageDown: 'pageDown',
  '?': 'help',
  m: 'menu',
  '/': 'request',
  s: 'settings',
}

const ARROWS: Record<string, Dir> = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' }

/** Whether the caret would leave a text field going left or right: until it
 *  reaches the end, left and right move the caret, not the focus. */
function caretAtEdge(el: Element, dir: Dir): boolean {
  if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) return true
  if (dir === 'up' || dir === 'down') return true
  const at = dir === 'left' ? el.selectionStart : el.selectionEnd
  if (at === null) return true
  return dir === 'left' ? at === 0 : at === el.value.length
}

/** A page down (or up) a long list: scroll by most of a screen, then focus
 *  whatever is now where the focus was. */
function page(down: boolean) {
  const current = document.activeElement as HTMLElement | null
  const y = current && current !== document.body ? current.getBoundingClientRect().top : window.innerHeight / 3
  window.scrollBy({ top: (down ? 1 : -1) * window.innerHeight * 0.8, behavior: 'auto' })
  const onScreen = focusables(scopeRoot()).filter((el) => {
    const r = el.getBoundingClientRect()
    return r.bottom > 0 && r.top < window.innerHeight
  })
  const near = onScreen.sort((a, b) => Math.abs(a.getBoundingClientRect().top - y) - Math.abs(b.getBoundingClientRect().top - y))[0]
  if (near) focusElement(near)
}

/** The big-screen layout: a top bar with big screen's pages, the
 *  page, and the input that drives it. One focus moves with the arrow keys,
 *  a D-pad or a stick; Enter or A chooses; Escape, Backspace or B closes
 *  what's open, then goes back. The root size changes while it shows (see
 *  index.css), so every big-screen page is sized in rem. `bare` (the first
 *  run) leaves the pages out: no tabs, and Back only closes what's
 *  open. The big-screen tour (`BigTour`) opens over any page that's asked
 *  for it (`tourState`). */
export function BigShell({ bare = false }: { bare?: boolean }) {
  useThemeSync()
  const navigate = useNavigate()
  const location = useLocation()
  const options = useBigScreenOptions()
  const switches = usePlayerSwitches()
  // In a window, the header is the window's title bar too.
  const drag = useWindowDrag()
  const [seen, setSeen] = useState<PadStyle | null>(null)
  const [help, setHelp] = useState(false)
  // Big setup's last step asks for the tour on its way here: it opens with
  // the page, and the ask is taken out of the history entry so going back
  // to it doesn't open it again.
  const askedForTour = !!(location.state as { bigTour?: unknown } | null)?.bigTour
  const [tour, setTour] = useState(askedForTour)
  const style = options.buttons === 'auto' ? seen : options.buttons
  useEffect(() => {
    if (askedForTour) navigate(location.pathname + location.search, { replace: true, state: null })
  }, [askedForTour, navigate, location.pathname, location.search])

  useEffect(() => {
    const root = document.documentElement
    root.dataset.bigscreen = ''
    root.style.setProperty('--big-ring', `${RING.width}rem`)
    root.style.setProperty('--big-ring-offset', `${RING.offset}rem`)
    const untrack = trackGroups()
    return () => {
      delete root.dataset.bigscreen
      root.style.removeProperty('--big-ring')
      root.style.removeProperty('--big-ring-offset')
      untrack()
    }
  }, [])

  // The focus is never left on nothing: a new page starts on its first item
  // once it has drawn, and when the focused item goes away (a card taken off
  // the lineup, a request folded away) the focus moves to the page's first
  // item again. A focus placed this way follows the page while it loads (the
  // lineup arriving after the top bar), until the person moves it.
  const placed = useRef<Element | null>(null)
  useEffect(() => {
    let frame = 0
    const settle = () => {
      frame = 0
      const el = document.activeElement
      const lost = !el || el === document.body || !el.isConnected
      if (!lost && el !== placed.current) return
      const first = start()
      if (first && first !== el) {
        focusElement(first)
        placed.current = first
      }
    }
    // A page with nothing to focus (an empty list, Help me decide before a
    // lineup) starts on its own tab: the first focusable is then the top
    // bar's first tab, which left the ring on Curate while another page
    // showed (found in a device test on Linux).
    const start = () => {
      const first = firstItem()
      if (!first || first.closest('main') || first.hasAttribute('data-focus-first') || scopeRoot() !== document.body) return first
      return document.querySelector<HTMLElement>('nav[aria-label="Big screen"] a[aria-current="page"]') ?? first
    }
    const later = () => {
      if (!frame) frame = requestAnimationFrame(settle)
    }
    const moved = (e: FocusEvent) => {
      if (e.target !== placed.current) placed.current = null
    }
    later()
    const watch = new MutationObserver(later)
    watch.observe(document.body, { childList: true, subtree: true })
    document.addEventListener('focusin', moved)
    return () => {
      watch.disconnect()
      document.removeEventListener('focusin', moved)
      if (frame) cancelAnimationFrame(frame)
    }
  }, [])

  // A page chosen from the top bar, or with LB and RB, starts on its own
  // first item too, rather than leaving the focus on a tab. In a layout
  // effect, so the tab is marked before the new page's first draw is
  // settled: a later effect would mark it once that had passed, and a page
  // with nothing more to draw would keep the focus on the tab.
  useLayoutEffect(() => {
    const el = document.activeElement
    if (el && !el.closest('main')) placed.current = el
  }, [location.pathname])

  const back = () => {
    if (closeTop()) return
    if (bare || location.pathname === '/') return
    // Back a page within UpNext, else home: never off the app.
    if ((window.history.state as { idx?: number } | null)?.idx) navigate(-1)
    else navigate('/')
  }

  const tab = (step: number) => {
    const i = BIG_TABS.findIndex((t) => (t.end ? location.pathname === t.to : location.pathname.startsWith(t.to)))
    const next = BIG_TABS[(Math.max(i, 0) + step + BIG_TABS.length) % BIG_TABS.length]
    navigate(next.to)
  }

  const act = (a: PadAction) => {
    if (offerAction(a)) return
    // The first run has no pages to go to.
    if (bare && (a === 'request' || a === 'prevTab' || a === 'nextTab' || a === 'settings')) return
    switch (a) {
      case 'up':
      case 'down':
      case 'left':
      case 'right':
        move(a)
        break
      case 'accept': {
        const el = document.activeElement as HTMLElement | null
        if (el && el !== document.body) el.click()
        else focusFirst()
        break
      }
      case 'back':
        back()
        break
      case 'request':
        navigate('/', { state: { request: Date.now() } })
        break
      case 'prevTab':
        tab(-1)
        break
      case 'nextTab':
        tab(1)
        break
      case 'pageUp':
        page(false)
        break
      case 'pageDown':
        page(true)
        break
      case 'settings':
        navigate('/settings')
        break
      case 'help':
        setHelp((h) => !h)
        break
      case 'menu':
        // Only a card has a menu, and it took the action above.
        break
    }
  }

  useGamepad((a, pad) => {
    setSeen(padStyle(pad.id))
    act(a)
  })

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return
      const typing = typingInField(e.target)
      const dir = ARROWS[e.key]
      if (dir) {
        if (typing && !caretAtEdge(e.target as Element, dir)) return
        e.preventDefault()
        if (offerAction(dir)) return
        move(dir)
        return
      }
      if (e.key === 'Escape' || (e.key === 'Backspace' && !typing)) {
        e.preventDefault()
        if (!offerAction('back')) back()
        return
      }
      if (e.key === 'Enter' && !typing) {
        // Enter clicks buttons and links by itself; a checkbox it doesn't.
        const el = document.activeElement
        if (!el || el === document.body) {
          e.preventDefault()
          focusFirst()
        } else if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) {
          e.preventDefault()
          el.click()
        }
        return
      }
      const a = typing ? undefined : KEY_ACTIONS[e.key]
      if (a) {
        e.preventDefault()
        act(a)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  return (
    <PadStyleContext.Provider value={style}>
      <div className={cx('flex min-h-full flex-col', options.overscan && 'p-[4vmin]')}>
        <header {...drag} className="sticky top-(--titlebar-h) z-30 flex min-h-[4rem] shrink-0 items-center gap-[1.5rem] border-b-[0.08rem] border-line bg-ground px-[2rem]">
          <span className="flex shrink-0 items-center gap-[0.5rem] text-ink">
            <PlayMark className="h-[1.6rem] w-[1.6rem]" />
            <span className="font-display text-[1.4rem] font-[750] tracking-[-0.02em]">UpNext</span>
          </span>
          {!bare && (
            <>
              <Glyph action="prevTab" />
              {/* Padding wider than the focus ring and its offset, since a
                  strip that scrolls sideways cuts off what's outside it. */}
              <nav
                aria-label="Big screen"
                className="flex min-w-0 gap-[0.25rem] overflow-x-auto [scrollbar-width:none]"
                style={{ padding: RING_ROOM }}
              >
                {BIG_TABS.map((t) => (
                  <NavLink
                    key={t.to}
                    to={t.to}
                    end={t.end}
                    className={({ isActive }) =>
                      cx(
                        'flex min-h-[3rem] shrink-0 items-center rounded-[0.7rem] px-[1rem] text-[1.05rem]',
                        isActive ? 'bg-raised font-semibold text-ink' : 'text-muted',
                      )
                    }
                  >
                    {t.label}
                  </NavLink>
                ))}
              </nav>
              <Glyph action="nextTab" />
            </>
          )}
          <div className="flex-1" />
          {!bare && <BigPlayerButton />}
          <button type="button" onClick={() => setHelp(true)} className="flex min-h-[3rem] shrink-0 items-center gap-[0.5rem] rounded-[0.7rem] px-[0.75rem] text-[1rem] text-muted">
            <Glyph action="help" />
            Buttons
          </button>
          <WindowControls className="-mr-[2rem] self-stretch" />
        </header>
        <main className="flex-1 px-[2rem] pb-[3rem]">
          {/* Each player gets the page afresh. */}
          <Outlet key={switches} />
        </main>
      </div>
      {help && (
        <ButtonHelp
          onClose={() => setHelp(false)}
          onTour={
            bare
              ? undefined
              : () => {
                  setHelp(false)
                  setTour(true)
                }
          }
        />
      )}
      {tour && <BigTour onClose={() => setTour(false)} />}
    </PadStyleContext.Provider>
  )
}

const HELP: [PadAction, string][] = [
  ['accept', 'Choose'],
  ['back', 'Back, or close what’s open'],
  ['menu', 'More for the focused game'],
  ['request', 'Change the request'],
  ['prevTab', 'Page before'],
  ['nextTab', 'Page after'],
  ['pageUp', 'Up a screen'],
  ['pageDown', 'Down a screen'],
  ['settings', 'Settings'],
  ['help', 'This help'],
]

/** What each button does, the tour, and the way out of big screen. */
function ButtonHelp({ onClose, onTour }: { onClose: () => void; onTour?: () => void }) {
  return (
    <BigSheet title="Buttons" onClose={onClose} className="max-w-[40rem]">
      <p className="m-0 text-[1rem] text-muted">The D-pad, the left stick or the arrow keys move between things.</p>
      <dl className="m-0 grid grid-cols-[auto_1fr] items-center gap-x-[1rem] gap-y-[0.6rem] text-[1.05rem]">
        {HELP.map(([a, what]) => (
          <div key={a} className="contents">
            <dt>
              <Glyph action={a} />
            </dt>
            <dd className="m-0">{what}</dd>
          </div>
        ))}
      </dl>
      <div className="flex flex-wrap gap-[0.75rem]">
        {onTour && (
          <button type="button" className="big-btn-outline" onClick={onTour}>
            Show me around
          </button>
        )}
        <button type="button" className="big-btn-outline" onClick={() => setBigScreen(false)}>
          Leave big screen
        </button>
        {inApp() && (
          // A full-screen window has no title bar to close it from.
          <button type="button" className="big-btn-outline" onClick={() => windowAction('close')}>
            Close UpNext
          </button>
        )}
        <button type="button" className="big-btn-outline" onClick={onClose}>
          Close
        </button>
      </div>
      <div className="flex justify-end">
        <Hint action="back">Close</Hint>
      </div>
    </BigSheet>
  )
}
