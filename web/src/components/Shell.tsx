import { useEffect, useRef, useState } from 'react'
import { Link, NavLink, Outlet, useNavigate } from 'react-router'
import type { Job, TaskId } from '../types'
import { cx, relativeTime } from '../lib/format'
import { jobName, jobVerb, lanesText, tookText, troubleText, unseenTrouble, waitingLane } from '../lib/jobs'
import { useTaskLabels } from '../lib/useTaskLabels'
import { useThemeSync } from '../lib/theme'
import { shortcutBlocked } from '../lib/keys'
import { frameOs } from '../lib/frame'
import { ActivityProvider } from './Activity'
import { LibrarySearch } from './LibrarySearch'
import { ProgressBar } from './ProgressBar'
import { useActivity } from './useActivity'
import { Tour } from './tour/Tour'
import { WindowControls } from './WindowFrame'
import { useWindowDrag } from '../lib/windowDrag'
import { usePlayerSwitches, usePlayers } from '../lib/players'
import { PlayerMenu } from './players/PlayerMenu'
import { NameNudge } from './players/NameNudge'
import { useJobs, useProfile } from '../lib/queries'

const TABS = [
  { to: '/', label: 'Pick', key: '1', end: true },
  { to: '/library', label: 'Library', key: '2' },
  { to: '/collections', label: 'Collections', key: '3' },
  { to: '/lists', label: 'Lists', key: '4' },
  { to: '/history', label: 'History', key: '5' },
]

/** UpNext's logo mark: the arc and dot of the app's icon (`web/public/favicon.svg`)
 *  without its tile, so it sits on the bar's own background. */
export function LogoMark({ className = 'h-[22px] w-[22px]' }: { className?: string }) {
  return (
    <svg viewBox="5 6 22 22" className={className} aria-hidden="true">
      <path d="M8 20a8 8 0 1 1 16 0" fill="none" stroke="var(--accent)" strokeWidth="3" strokeLinecap="round" />
      <circle cx="16" cy="21" r="2.5" fill="var(--accent)" />
    </svg>
  )
}

/** The play triangle, on Play buttons. */
export function PlayMark({ className = 'h-[22px] w-[22px]', fill = 'var(--accent)' }: { className?: string; fill?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      <path d="M7 4.5c0-1.1 1.2-1.8 2.2-1.2l10.3 6.7c.9.6.9 1.9 0 2.5L9.2 19.2c-1 .6-2.2-.1-2.2-1.2z" fill={fill} />
    </svg>
  )
}

function SlidersIcon() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" />
      <circle cx="15" cy="6" r="2" />
      <circle cx="9" cy="12" r="2" />
      <circle cx="17" cy="18" r="2" />
    </svg>
  )
}


/** Where the top bar has room for the sync line in full: from 1760 px, past
 *  the search box at its widest and the profile name (the e2e test measures it). */
const FULL_SYNC = 'hidden min-[1760px]:flex'
const DOT_SYNC = 'flex min-[1760px]:hidden'

/** "Synced 4m ago", or a running job's count and bar; it opens Activity. */
function SyncStatus() {
  const activity = useActivity()
  const labels = useTaskLabels()
  // Polled by Activity's job watcher; this only reads it.
  const jobs = useJobs()
  const profile = useProfile()
  const job = jobs.data?.current?.status === 'running' ? jobs.data.current : null
  const trouble = job ? null : unseenTrouble(jobs.data?.recent, activity.seenId)
  const announce = useJobAnnouncement(jobs.data !== undefined, job, labels)

  // Same number Settings shows: the last full sync.
  const lastSync =
    profile.data?.last_sync ??
    profile.data?.sources.reduce<number | null>((m, s) => (s.last_sync && (!m || s.last_sync > m) ? s.last_sync : m), null)

  let text: string
  let dot: string
  // The jobs poll runs every 15s, so it notices a stopped server first.
  const offline = jobs.isError
  const waiting = job ? waitingLane(job) : null
  if (offline) {
    text = "Can't reach UpNext"
    dot = 'bg-bad'
  } else if (job) {
    const lanes = lanesText(job)
    text = lanes
      ? `${jobVerb(job)} · ${lanes}`
      : job.total > 0
        ? `${jobVerb(job)} ${job.done.toLocaleString()} / ${job.total.toLocaleString()}`
        : `${jobVerb(job)}…`
    if (job.eta_secs != null && job.eta_secs > 0) text += ` · ${tookText(job.eta_secs)} left`
    dot = cx('animate-pulse', waiting ? 'bg-warn' : 'bg-accent')
  } else if (trouble) {
    const name = jobName(trouble, labels)
    text = troubleText(trouble, name)
    dot = trouble.status === 'failed' ? 'bg-bad' : 'bg-warn'
  } else {
    text = lastSync ? `Synced ${relativeTime(lastSync)}` : 'Not synced yet'
    dot = lastSync ? 'bg-good' : 'bg-faint'
  }
  const more = job
    ? [jobName(job, labels), job.source === 'terminal' && 'from a terminal', waiting?.note].filter(Boolean).join(' · ')
    : trouble
      ? (trouble.error ?? trouble.summary ?? undefined)
      : undefined
  // Two forms, so the top bar never moves or clips it: where there
  // is room, the whole line in a box of fixed width; below that, only the
  // dot, with the line as its name and tooltip. Between 1024 and 1279 px the
  // search box takes its room.
  return (
    <>
      <button
        type="button"
        onClick={() => (activity.isOpen ? activity.close() : activity.open())}
        aria-haspopup="dialog"
        aria-expanded={activity.isOpen}
        aria-label={text}
        title={[text, more].filter(Boolean).join(' · ')}
        className="hidden h-11 shrink-0 flex-col justify-center gap-1 text-[13px] text-muted hover:text-ink md:flex lg:hidden xl:flex"
      >
        <span className={cx(FULL_SYNC, 'w-[270px] items-center gap-2')}>
          <span aria-hidden="true" className={cx('h-[7px] w-[7px] shrink-0 rounded-full', dot)} />
          <span className="truncate tabular-nums">{text}</span>
        </span>
        {job && (
          <span aria-hidden="true" className={cx(FULL_SYNC, 'w-[270px]')}>
            <ProgressBar job={job} label="" className="w-full" />
          </span>
        )}
        {trouble?.status === 'failed' && <span aria-hidden="true" className={cx(FULL_SYNC, 'h-1 w-[270px] rounded-sm bg-bad')} />}
        <span aria-hidden="true" className={cx(DOT_SYNC, 'h-11 w-6 items-center justify-center')}>
          <span className={cx('h-[9px] w-[9px] rounded-full', dot)} />
        </span>
      </button>
      <span className="sr-only" aria-live="polite">
        {announce}
      </span>
    </>
  )
}

/** A polite line for screen readers when a job starts or ends, never per tick. A job
 *  already running when the page loads isn't announced as starting. */
function useJobAnnouncement(loaded: boolean, job: Job | null, labels: Record<TaskId, string>): string {
  const [text, setText] = useState('')
  const last = useRef<{ id: number; name: string } | null | undefined>(undefined)
  const id = job?.id ?? null
  const name = job ? jobName(job, labels) : null
  useEffect(() => {
    if (!loaded) return
    const first = last.current === undefined
    if (id != null && last.current?.id !== id) {
      last.current = { id, name: name ?? '' }
      if (!first) setText(`${name} started`)
    } else if (id == null && last.current) {
      setText(`${last.current.name} ended`)
      last.current = null
    } else if (first) {
      last.current = null
    }
  }, [loaded, id, name])
  return text
}

/** Your Steam name and picture, opening your taste profile. With two or
 *  more players, the player menu in its place. */
function ProfileLink() {
  const { multi } = usePlayers()
  if (multi) return <PlayerMenu className="hidden xl:block" />
  return <SteamProfileLink />
}

function SteamProfileLink() {
  const profile = useProfile()
  const name = profile.data?.persona_name
  return (
    <NavLink
      to="/profile"
      aria-label={name ? `Your profile (${name})` : 'Your profile'}
      className={({ isActive }) =>
        // From 1280 px, where the narrower search box makes room for it; the
        // top bar is full below that, and Settings › Profile opens it there.
        cx('hidden h-11 shrink-0 items-center gap-2 rounded-[10px] px-3 text-[14.5px] xl:flex', isActive ? 'bg-raised font-semibold text-ink' : 'text-muted hover:text-ink')
      }
    >
      {profile.data?.avatar ? (
        <img src={profile.data.avatar} alt="" width={24} height={24} className="h-6 w-6 shrink-0 rounded-full object-cover" />
      ) : (
        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
          <circle cx="12" cy="8" r="4" />
          <path d="M4 21c1.5-4 4.5-6 8-6s6.5 2 8 6" />
        </svg>
      )}
      <span className="hidden max-w-[140px] truncate 2xl:inline">{name ?? 'Profile'}</span>
    </NavLink>
  )
}

export function Shell() {
  return (
    <ActivityProvider>
      <Layout />
    </ActivityProvider>
  )
}

function Layout() {
  useThemeSync()
  const navigate = useNavigate()
  const switches = usePlayerSwitches()
  const drag = useWindowDrag()
  const framed = !!frameOs()

  // 1-5 switch tabs when not typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (shortcutBlocked(e)) return
      const tab = TABS.find((t) => t.key === e.key)
      if (tab) {
        e.preventDefault()
        navigate(tab.to)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [navigate])

  return (
    <div className="flex min-h-full flex-col">
      {/* Below 640 px the name and the gaps give way, and the tabs scroll
          sideways if they still don't fit, so the page keeps its width.
          Below 1024 px the tabs' shortcut keys and the Settings label go
          and the gaps narrow, and the wider gaps wait for 1280 px and the
          widest for 1920 px, so the bar fits the window at every width.
          In the desktop app it is also the window's title bar: it drags the
          window, and the window's buttons end it. To leave them
          room, the name waits for 1536 px and Settings' label for 1280 px,
          and below 1024 px the tabs may scroll. */}
      <header data-tour="topbar" {...drag} className="sticky top-(--titlebar-h) z-30 flex h-16 shrink-0 items-center gap-2 border-b border-line bg-ground px-4 sm:gap-4 sm:px-10 lg:gap-6 xl:gap-8 min-[1920px]:gap-11">
        <Link to="/" className="flex shrink-0 items-center gap-[9px] text-ink" aria-label="UpNext home">
          <LogoMark />
          <span className={cx('hidden font-display text-[21px] font-[750] tracking-[-0.02em]', framed ? '2xl:inline' : 'sm:inline')}>UpNext</span>
        </Link>
        <nav
          aria-label="Main"
          className={cx(
            'flex h-full gap-0.5 text-[14px] sm:gap-1.5 sm:text-[15px]',
            framed ? 'max-lg:min-w-0 max-lg:overflow-x-auto max-lg:[scrollbar-width:none]' : 'max-sm:min-w-0 max-sm:overflow-x-auto max-sm:[scrollbar-width:none]',
          )}
        >
          {TABS.map((t) => (
            <NavLink
              key={t.to}
              to={t.to}
              end={t.end}
              className={({ isActive }) =>
                cx(
                  'group flex shrink-0 items-center gap-2 px-1.5 sm:px-2.5 lg:px-3.5',
                  isActive ? 'font-semibold text-ink shadow-[inset_0_-2px_0_var(--accent)]' : 'text-muted hover:text-ink',
                )
              }
            >
              {({ isActive }) => (
                <>
                  <span aria-hidden="true" className={cx('hidden font-mono text-[10px] lg:inline', isActive ? 'text-accent' : 'text-faint')}>{t.key}</span>
                  {/* As wide as the label in bold, so the active tab's weight moves nothing after it. */}
                  <span className="grid">
                    <span className="col-start-1 row-start-1">{t.label}</span>
                    <span aria-hidden="true" className="invisible col-start-1 row-start-1 font-semibold">
                      {t.label}
                    </span>
                  </span>
                </>
              )}
            </NavLink>
          ))}
        </nav>
        <div className="flex-1" />
        {/* Where the bar is full it narrows to fit the window rather than widen the page. */}
        <LibrarySearch place="topbar" className="hidden h-[38px] w-[272px] min-w-0 lg:flex 2xl:w-[320px]" />
        <SyncStatus />
        <ProfileLink />
        <NavLink
          to="/settings"
          className={({ isActive }) =>
            cx(
              'flex h-11 shrink-0 items-center gap-2 rounded-[10px] px-2 text-[14.5px] sm:px-3',
              isActive ? 'bg-raised font-semibold text-ink' : 'text-muted hover:text-ink',
            )
          }
        >
          <SlidersIcon />
          {/* Still the link's name where only the icon shows. */}
          <span className={framed ? 'sr-only xl:not-sr-only' : 'sr-only lg:not-sr-only'}>Settings</span>
        </NavLink>
        {/* Out to the window's corner, past the bar's padding. */}
        <WindowControls className="-mr-4 self-stretch sm:-mr-10" />
      </header>
      <Tour />
      <NameNudge />
      <main className="flex-1">
        {/* Each player gets the page afresh. */}
        <Outlet key={switches} />
      </main>
    </div>
  )
}
