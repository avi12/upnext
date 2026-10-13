import { useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { cx } from '../lib/format'
import { shortcutBlocked } from '../lib/keys'
import { useProfile } from '../lib/queries'

/** What the Library is opened with when / is pressed where no search box shows. */
export interface SearchState {
  focusSearch?: boolean
}

/** Whether the element is laid out, so a box hidden at this width is left alone. */
function shown(el: HTMLElement | null): el is HTMLElement {
  return !!el && el.getClientRects().length > 0
}

/** Library search by name. The top bar has one from 1024 px; below that the
 *  Library page shows its own, and only the one on screen answers /. On the
 *  Library it filters live through ?q=; anywhere else Enter jumps to the
 *  Library with the query. */
export function LibrarySearch({ place, className }: { place: 'topbar' | 'library'; className?: string }) {
  const navigate = useNavigate()
  const location = useLocation()
  const [params, setParams] = useSearchParams()
  const onLibrary = location.pathname.startsWith('/library')
  const [value, setValue] = useState(onLibrary ? (params.get('q') ?? '') : '')
  const ref = useRef<HTMLInputElement>(null)
  const profile = useProfile()

  // Follow ?q= when it changes elsewhere (e.g. a "search this tag" link, or
  // the other box); off the Library the box starts empty again.
  useEffect(() => {
    setValue(onLibrary ? (params.get('q') ?? '') : '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onLibrary, params.get('q')])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== '/' || shortcutBlocked(e)) return
      if (shown(ref.current)) {
        e.preventDefault()
        ref.current.focus()
        ref.current.select()
      } else if (place === 'topbar' && !onLibrary) {
        // Below 1024 px only the Library has a box, so / goes there.
        e.preventDefault()
        navigate('/library', { state: { focusSearch: true } satisfies SearchState })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [place, onLibrary, navigate])

  const focusSearch = place === 'library' && (location.state as SearchState | null)?.focusSearch === true
  useEffect(() => {
    if (focusSearch && shown(ref.current)) ref.current.focus()
  }, [focusSearch])

  const update = (v: string) => {
    setValue(v)
    if (onLibrary) {
      const next = new URLSearchParams(params)
      if (v.trim()) next.set('q', v)
      else next.delete('q')
      setParams(next, { replace: true })
    }
  }

  return (
    <form
      role="search"
      onSubmit={(e) => {
        e.preventDefault()
        if (!onLibrary) navigate(`/library${value.trim() ? `?q=${encodeURIComponent(value.trim())}` : ''}`)
        ref.current?.blur()
      }}
      className={cx('items-center gap-2.5 rounded-lg bg-raised px-3 text-sm text-dim', className)}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
        <circle cx="11" cy="11" r="7" />
        <path d="M20 20l-3.5-3.5" />
      </svg>
      <input
        ref={ref}
        type="search"
        value={value}
        onChange={(e) => update(e.target.value)}
        onKeyDown={(e) => e.key === 'Escape' && (update(''), ref.current?.blur())}
        placeholder={profile.data ? `Find a game in ${profile.data.game_count.toLocaleString()}…` : 'Find a game…'}
        aria-label="Search your library"
        className="h-full min-w-0 flex-1 bg-transparent text-ink outline-none placeholder:text-dim"
      />
      {/* Phones have no / key to press. */}
      <kbd className="hidden rounded border border-key px-[5px] py-px font-mono text-[10px] text-dim sm:inline">/</kbd>
    </form>
  )
}
