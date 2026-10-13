import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { CheckCircle2, AlertTriangle, Info, X } from 'lucide-react'
import { cx } from '../lib/format'
import { typingInField } from '../lib/keys'
import { ACTION_KEYS, ACTION_KEYS_SAID, ToastContext, type ToastApi, type ToastItem } from './useToast'

let nextId = 1

/** How long a toast shows. One with a button (an Undo, say) gets as long as
 *  an error, so a keyboard or screen-reader user can reach it, and none
 *  closes while the pointer or the focus is on it. */
const SHORT_MS = 4500
const LONG_MS = 8000
/** The least a toast shows once the pointer or the focus has left it. */
const AFTER_LEAVING_MS = 2000

/** A toast as shown: with where the focus was when it came, to go back to
 *  if the focus is on the toast when it goes. */
type Shown = ToastItem & { from: HTMLElement | null }

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Shown[]>([])
  const latest = useRef(items)
  useEffect(() => {
    latest.current = items
  })

  const dismiss = useCallback((id: number) => {
    setItems((s) => s.filter((t) => t.id !== id))
  }, [])

  const push = useCallback(
    (t: Omit<ToastItem, 'id'>) => {
      const id = nextId++
      const from = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null
      setItems((s) => [...s.slice(-3), { ...t, id, from }])
    },
    [],
  )

  const api = useMemo<ToastApi>(
    () => ({
      push,
      success: (title, detail, action) => push({ kind: 'success', title, detail, action }),
      error: (title, detail, action) => push({ kind: 'error', title, detail, action }),
      info: (title, detail, action) => push({ kind: 'info', title, detail, action }),
    }),
    [push],
  )

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== 'z' || !(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return
      // A field's own undo, and a dialog over the page, come first.
      if (typingInField(e.target) || document.querySelector('[aria-modal="true"]')) return
      const t = latest.current.findLast((t) => t.action)
      if (!t?.action) return
      e.preventDefault()
      t.action.onClick()
      dismiss(t.id)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [dismiss])

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div role="status" aria-live="polite" data-toasts className="pointer-events-none fixed bottom-20 right-4 z-[100] flex w-[min(92vw,380px)] flex-col gap-2 md:bottom-6">
        {items.map((t) => (
          <ToastCard key={t.id} t={t} dismiss={dismiss} />
        ))}
      </div>
    </ToastContext.Provider>
  )
}

function ToastCard({ t, dismiss }: { t: Shown; dismiss: (id: number) => void }) {
  const card = useRef<HTMLDivElement>(null)
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const left = useRef(t.kind === 'error' || t.action ? LONG_MS : SHORT_MS)
  // The clock runs only while nobody is on the toast, and goes on from
  // where it stopped.
  useEffect(() => {
    if (hovered || focused) return
    const started = Date.now()
    const timer = window.setTimeout(() => dismiss(t.id), left.current)
    return () => {
      window.clearTimeout(timer)
      left.current = Math.max(AFTER_LEAVING_MS, left.current - (Date.now() - started))
    }
  }, [hovered, focused, dismiss, t.id])

  // Closed with the focus on it, the toast gives the focus back to where it
  // was when the toast came, or else to where it came from into the toast,
  // rather than leaving it on the page's body.
  const entered = useRef<HTMLElement | null>(null)
  const close = () => {
    const back = [t.from, entered.current].find((el) => el?.isConnected && !card.current?.contains(el))
    const inside = !!card.current?.contains(document.activeElement)
    dismiss(t.id)
    if (inside) back?.focus()
  }

  return (
    <div
      ref={card}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      onFocus={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) entered.current = e.relatedTarget instanceof HTMLElement ? e.relatedTarget : null
        setFocused(true)
      }}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setFocused(false)
      }}
      className={cx(
        'pointer-events-auto fade-up flex items-start gap-3 rounded-xl border border-line-strong bg-sheet px-4 py-3 shadow-2xl',
        t.kind === 'error' && 'border-bad/40',
        t.kind === 'success' && 'border-good/30',
      )}
    >
      {t.kind === 'success' && <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-good" />}
      {t.kind === 'error' && <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-bad" />}
      {t.kind === 'info' && <Info className="mt-0.5 h-4 w-4 shrink-0 text-accent" />}
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium">{t.title}</div>
        {t.detail && <div className="mt-0.5 text-xs text-muted break-words">{t.detail}</div>}
        {/* Said with the message when the toast comes: the action and its
            keys, which a screen reader skipped when only the button named it
            (Orca in Brave read the message alone). */}
        {t.action && (
          <span className="sr-only">
            {' '}
            {t.action.label}: press {ACTION_KEYS_SAID}.
          </span>
        )}
        {t.action && (
          <button
            type="button"
            className="mt-1.5 text-xs font-semibold text-accent hover:underline"
            aria-keyshortcuts={ACTION_KEYS}
            title={ACTION_KEYS_SAID}
            onClick={() => {
              t.action?.onClick()
              close()
            }}
          >
            {t.action.label}
          </button>
        )}
      </div>
      <button
        className="rounded-md p-1 text-dim hover:bg-raised hover:text-ink"
        onClick={close}
        aria-label="Dismiss"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}
