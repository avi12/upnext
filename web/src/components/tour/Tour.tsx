import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useLocation, useNavigate, useSearchParams } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView, SetupChange } from '../../types'
import { GameSheet } from '../GameSheet'
import { showLineup } from '../pick/useGameActions'
import { TOUR_STOPS } from './stops'
import { titleBarHeight } from '../../lib/frame'
import { keys, useConfig, useSetupState } from '../../lib/queries'

/** The tour: about ten stops, each a card next to a
 *  highlighted part of the real UI with the rest dimmed. `?tour=1` on any
 *  page starts it (Settings › Accounts and the wizard's last step link
 *  there); an install that has never taken or turned it down gets a
 *  one-time line offering it. Lives in the shell, so it follows the tabs. */
export function Tour() {
  const qc = useQueryClient()
  const [params] = useSearchParams()
  const navigate = useNavigate()
  const state = useSetupState()
  const config = useConfig()
  // A library filled from a terminal skips the wizard, and with it the AI
  // step: with no AI set up, the offer line points there too.
  const noAi = config.data != null && !config.data.claude_available
  const [at, setAt] = useState<number | null>(null)
  // Where focus was before, to go back to afterwards.
  const before = useRef<HTMLElement | null>(null)
  const { mutate: record } = useMutation({
    mutationFn: (change: SetupChange) => api.updateSetup(change),
    onSuccess: (s) => qc.setQueryData(keys.setupState, s),
  })

  const start = useCallback(() => {
    before.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    setAt(0)
  }, [])

  // `?tour=1` on any page starts it; the address becomes the first stop's.
  const asked = params.has('tour')
  const [wasAsked, setWasAsked] = useState(false)
  if (asked !== wasAsked) {
    setWasAsked(asked)
    if (asked) setAt(0)
  }
  useEffect(() => {
    if (asked) navigate(TOUR_STOPS[0].path, { replace: true })
  }, [asked, navigate])

  const tour = state.data?.tour
  const end = useCallback(
    (finished: boolean) => {
      setAt(null)
      // Leaving early doesn't undo a tour taken before.
      if (finished) record({ tour: 'done' })
      else if (tour !== 'done') record({ tour: 'skipped' })
      const el = before.current
      before.current = null
      // Back where it started; when that's gone (the offer line's button,
      // a link on the page the tour left), to the top bar's first link.
      requestAnimationFrame(() => {
        const to = el?.isConnected ? el : document.querySelector<HTMLElement>('[data-tour="topbar"] a')
        to?.focus()
      })
    },
    [record, tour],
  )

  if (at !== null) return <Running at={at} go={setAt} end={end} />
  if (!state.data?.tour_offer) return null
  return (
    <div role="region" aria-label="Tour" className="flex flex-wrap items-center gap-x-5 gap-y-1 border-b border-line px-4 py-2 text-[14.5px] text-muted sm:px-10">
      <span>A two-minute tour shows where everything is in UpNext, and which words are menus.</span>
      <span className="flex flex-wrap gap-x-5">
        <button type="button" onClick={start} className="min-h-10 font-semibold text-ink underline decoration-underline underline-offset-4 hover:decoration-accent">
          Show me around
        </button>
        {noAi && (
          <Link to="/setup?again=1" className="inline-flex min-h-10 items-center text-ink underline decoration-underline underline-offset-4 hover:decoration-accent">
            Run setup to add an AI
          </Link>
        )}
        <button type="button" onClick={() => record({ tour: 'skipped' })} className="btn-quiet min-h-10">
          No thanks
        </button>
      </span>
    </div>
  )
}

const anchor = (id: string) => document.querySelector<HTMLElement>(`[data-tour="${id}"]`)

/** How long a stop waits for its part of the page before showing its card
 *  without a highlight (a page still loading, or an empty one). */
const WAIT_MS = 4000
const MARGIN = 16
const GAP = 14
/** The top bar's height plus a little: where a scrolled-to part lands,
 *  below the desktop app's slim title bar too on a page that has one (the
 *  window's buttons sit in the top bar otherwise). */
const UNDER_BAR = 80

interface Box {
  top: number
  left: number
  width: number
  height: number
}

interface Layout {
  /** The highlight, or null to dim everything. */
  spot: Box | null
  card: { top: number; left: number }
  /** The stop whose part was found, or whose wait ran out: its card shows. */
  ready: string | null
}

/** Whether a card `w` by `h` fits beside the highlight around `r`, above
 *  or below it or to one side, wherever the page is scrolled. */
function roomBeside(r: DOMRect, w: number, h: number): boolean {
  return r.height + 12 + GAP + h <= window.innerHeight - 2 * MARGIN || r.width + 12 + GAP + w <= window.innerWidth - 2 * MARGIN
}

/** The highlight around `r`: a little bigger, but kept inside the window
 *  with room for its ring, so a full-width part (the top bar) shows it. */
function spotAround(r: DOMRect): Box {
  const top = Math.max(Math.round(r.top - 6), titleBarHeight() + 2)
  const left = Math.max(Math.round(r.left - 6), 2)
  const bottom = Math.min(Math.round(r.bottom + 6), window.innerHeight - 2)
  const right = Math.min(Math.round(r.right + 6), window.innerWidth - 2)
  return { top, left, width: right - left, height: bottom - top }
}

/** The card beside the highlight: below it, above, right or left, wherever
 *  it fits, else in the corner over it. Below 640 px it sits at the bottom. */
function place(spot: Box | null, w: number, h: number): Layout['card'] {
  const vw = window.innerWidth
  const vh = window.innerHeight
  // Clear of the desktop app's slim title bar where a page has one; the
  // window's buttons stay above the tour either way.
  const ceiling = titleBarHeight() + MARGIN
  if (vw < 640) return { left: MARGIN, top: vh - h - MARGIN }
  if (!spot) return { left: (vw - w) / 2, top: (vh - h) / 2 }
  const x = (v: number) => Math.min(Math.max(v, MARGIN), vw - w - MARGIN)
  const y = (v: number) => Math.min(Math.max(v, ceiling), vh - h - MARGIN)
  const bottom = spot.top + spot.height
  const right = spot.left + spot.width
  if (bottom + GAP + h <= vh - MARGIN) return { left: x(spot.left), top: bottom + GAP }
  if (spot.top - GAP - h >= ceiling) return { left: x(spot.left), top: spot.top - GAP - h }
  if (right + GAP + w <= vw - MARGIN) return { left: right + GAP, top: y(spot.top) }
  if (spot.left - GAP - w >= MARGIN) return { left: spot.left - GAP - w, top: y(spot.top) }
  return { left: vw - w - MARGIN, top: vh - h - MARGIN }
}

/** Something pinned to the screen (the top bar, the sheet) needs no scrolling. */
function pinned(el: HTMLElement): boolean {
  for (let e: HTMLElement | null = el; e; e = e.parentElement) {
    const p = getComputedStyle(e).position
    if (p === 'fixed' || p === 'sticky') return true
  }
  return false
}

/** Scroll the part into view: centred when it fits, else from its top, under
 *  the top bar. On a phone it goes to the top, clear of the card below. */
function bringIntoView(el: HTMLElement, smooth: boolean) {
  if (pinned(el)) return
  const r = el.getBoundingClientRect()
  const vh = window.innerHeight
  const narrow = window.innerWidth < 640
  const under = UNDER_BAR + titleBarHeight()
  if (r.top >= under && r.bottom <= (narrow ? vh * 0.55 : vh - MARGIN)) return
  const by = narrow || r.height > vh - under - MARGIN ? r.top - under : r.top - (vh - r.height) / 2
  window.scrollBy({ top: by, behavior: smooth ? 'smooth' : 'auto' })
}

/** The best-reviewed game with People say and Vibes, for the sheet stop. */
async function sheetGame(): Promise<GameView | null> {
  const r = await api.games({ sort: 'review', order: 'desc', limit: 60 })
  return r.items.find((g) => g.digest && g.vibes) ?? r.items.find((g) => g.digest || g.vibes) ?? r.items[0] ?? null
}

function Running({ at, go, end }: { at: number; go: (i: number) => void; end: (finished: boolean) => void }) {
  const stop = TOUR_STOPS[at]
  const last = at === TOUR_STOPS.length - 1
  const navigate = useNavigate()
  const qc = useQueryClient()
  const { pathname } = useLocation()
  const smooth = useMemo(() => !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches, [])
  const config = useConfig()
  const game = useQuery({ queryKey: ['tour', 'sheet-game'], queryFn: sheetGame, staleTime: 60_000 })
  const rootRef = useRef<HTMLDivElement>(null)
  const cardRef = useRef<HTMLDivElement>(null)
  const titleRef = useRef<HTMLHeadingElement>(null)
  const [layout, setLayout] = useState<Layout>({ spot: null, card: { top: 0, left: 0 }, ready: null })
  const [lineup, setLineup] = useState<string | null>(null)

  // The tour moves between tabs itself, checked at every stop.
  useEffect(() => {
    if (pathname !== stop.path) navigate(stop.path)
    // Only when the stop changes: going back in the browser isn't fought.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [at])

  // The sheet stop waits for its game; with none, it points at the covers.
  const target = stop.sheet ? (game.data ? stop.target : game.isFetched || game.isError ? (stop.fallback ?? stop.target) : null) : stop.target

  // Follow the part every frame: pages load, menus open, the page scrolls.
  useEffect(() => {
    if (!target) return
    const t0 = performance.now()
    let opened = !stop.open
    let found = false
    // What was last scrolled to: a new part (the first pick once a roll
    // fills the lineup) is brought into view too.
    let shown: Element | null = null
    let ready = false
    let seen = ''
    let raf = 0
    const tick = () => {
      if (!opened) {
        const toggle = anchor(stop.open!)
        if (toggle) {
          if (toggle.getAttribute('aria-expanded') === 'false') toggle.click()
          opened = true
        } else if (anchor(target)) opened = true
      }
      const whole = anchor(target)
      if (stop.example) setLineup(whole?.dataset.state ?? null)
      const card = cardRef.current
      // A part too big for the card to sit beside (the lineup in a laptop's
      // window) gives way to its first item; a phone's card sits below anyway.
      const first = stop.firstOf ? anchor(stop.firstOf)?.firstElementChild : null
      const wholeRect = whole?.getBoundingClientRect()
      const cramped =
        window.innerWidth >= 640 && !!wholeRect && wholeRect.height > 0 && !roomBeside(wholeRect, card?.offsetWidth ?? 0, card?.offsetHeight ?? 0)
      const el = cramped && first instanceof HTMLElement ? first : whole
      const r = el?.getBoundingClientRect()
      const visible = el && r && r.width > 0 && r.height > 0 ? r : null
      if (visible && el !== shown) {
        found = true
        shown = el
        bringIntoView(el!, smooth)
      }
      if (!ready && (found || performance.now() - t0 > WAIT_MS)) ready = true
      const spot = visible ? spotAround(visible) : null
      const pos = place(spot, card?.offsetWidth ?? 0, card?.offsetHeight ?? 0)
      const next: Layout = { spot, card: { top: Math.round(pos.top), left: Math.round(pos.left) }, ready: ready ? stop.id : null }
      const key = JSON.stringify(next)
      if (key !== seen) {
        seen = key
        setLayout(next)
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [stop, target, smooth])

  // The lineup stop with no lineup yet: an example from UpNext's own
  // ranking, with no AI call, so the stop has something to point at.
  // It's a lineup like any other, so it's in History too.
  const asked = useRef(false)
  const [example, setExample] = useState(false)
  useEffect(() => {
    if (!stop.example || lineup !== 'empty' || asked.current) return
    asked.current = true
    api.pick('', 3, [], undefined, [], true).then(
      (r) => {
        showLineup(qc, r)
        setExample(true)
      },
      // Nothing to pick from: the stop says where picks will show.
      () => {},
    )
  }, [stop.example, lineup, qc])

  // Focus goes to each stop's title once its card shows.
  const ready = layout.ready === stop.id
  useEffect(() => {
    if (ready) titleRef.current?.focus({ preventScroll: true })
  }, [ready])

  const next = useCallback(() => (last ? end(true) : go(at + 1)), [last, end, go, at])
  const back = useCallback(() => at > 0 && go(at - 1), [at, go])

  // Esc leaves, the arrows step, Tab stays in the card, and focus that
  // wanders off (the sheet takes it when it opens) comes back to it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        end(false)
      } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
        e.preventDefault()
        e.stopPropagation()
        if (e.key === 'ArrowRight') next()
        else back()
      } else if (e.key === 'Tab') {
        const items = [...(cardRef.current?.querySelectorAll<HTMLElement>('button:not([disabled])') ?? [])]
        if (!items.length) return
        const i = items.indexOf(document.activeElement as HTMLElement)
        const to = e.shiftKey ? (i <= 0 ? items.length - 1 : i - 1) : i === items.length - 1 ? 0 : i + 1
        e.preventDefault()
        items[to].focus()
      }
    }
    const onFocus = (e: FocusEvent) => {
      if (e.target instanceof Node && !rootRef.current?.contains(e.target)) titleRef.current?.focus({ preventScroll: true })
    }
    window.addEventListener('keydown', onKey, true)
    document.addEventListener('focusin', onFocus)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      document.removeEventListener('focusin', onFocus)
    }
  }, [end, next, back])

  const ctx = { lineup, ai: config.data?.claude_available === true, example, sheetGame: !!game.data }
  const { spot, card } = layout
  return (
    <>
      {stop.sheet && game.data && <GameSheet game={game.data} onClose={() => {}} />}
      <div ref={rootRef} role="dialog" aria-modal="true" aria-labelledby="tour-title" aria-describedby="tour-body" className="fixed inset-0 z-[200]">
        {/* The page stays still underneath: clicks land here. */}
        <div className="absolute inset-0" aria-hidden="true" style={spot ? undefined : { background: 'var(--backdrop)' }} />
        {spot && (
          <div
            aria-hidden="true"
            data-tour-spot=""
            className="pointer-events-none absolute rounded-[10px] transition-[top,left,width,height] duration-200 ease-out motion-reduce:transition-none"
            style={{ ...spot, boxShadow: '0 0 0 2px var(--accent), 0 0 0 200vmax var(--backdrop)' }}
          />
        )}
        <div
          ref={cardRef}
          key={stop.id}
          className="absolute w-[min(380px,calc(100vw-32px))] rounded-xl border border-line-strong bg-sheet p-5 shadow-[0_12px_36px_rgb(0_0_0/0.35)]"
          style={{ ...card, visibility: ready ? 'visible' : 'hidden' }}
        >
          <p className="m-0 font-mono text-[11px] text-dim">
            {at + 1} of {TOUR_STOPS.length}
          </p>
          <h2 id="tour-title" ref={titleRef} tabIndex={-1} className="m-0 mt-1.5 font-display text-[21px] leading-tight font-bold outline-none">
            {stop.title}
          </h2>
          <p id="tour-body" className="m-0 mt-2 text-[15px] leading-[1.55] text-muted">
            {stop.body(ctx)}
          </p>
          <div className="mt-4 flex items-center gap-3">
            <button type="button" onClick={() => end(false)} className="btn-quiet mr-auto min-h-10 text-sm underline decoration-underline underline-offset-4">
              Skip tour
            </button>
            {at > 0 && (
              <button type="button" onClick={back} className="btn-outline">
                Back
              </button>
            )}
            <button type="button" onClick={next} className="btn-accent h-10">
              {last ? 'Done' : 'Next'}
            </button>
          </div>
        </div>
      </div>
    </>
  )
}
