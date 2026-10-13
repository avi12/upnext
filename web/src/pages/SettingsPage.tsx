import { Fragment, useEffect, useMemo, useState } from 'react'
import { useLocation } from 'react-router'
import { useSignInReturn } from '../lib/useSignInReturn'
import { Skeleton } from '../components/Skeleton'
import { cx } from '../lib/format'
import { AccountsSection } from '../components/settings/Accounts'
import { SetupSection } from '../components/settings/Setup'
import { LibraryDataSection } from '../components/settings/LibraryData'
import { JobsSection } from '../components/settings/Jobs'
import { BackupsSection } from '../components/settings/Backups'
import { PicksSection } from '../components/settings/Picks'
import { AiUsageSection } from '../components/settings/AiUsage'
import { LooksSection } from '../components/settings/Looks'
import { ProfileSection } from '../components/settings/Profile'
import { DevicesSection } from '../components/settings/Devices'
import { AboutSection } from '../components/settings/About'
import { WindowsSection } from '../components/settings/Windows'
import { BigScreenSection } from '../components/settings/BigScreen'
import { useSettingsSave } from '../components/settings/useSettingsSave'
import { PlayersSection } from '../components/settings/Players'
import { SharingSection } from '../components/settings/Sharing'
import { PlayingAs } from '../components/players/PlayerMenu'
import { usePlayers } from '../lib/players'
import { useSettings } from '../lib/queries'

const SECTIONS = [
  { id: 'setup', label: 'Setup' },
  { id: 'accounts', label: 'Accounts' },
  { id: 'data', label: 'Library data' },
  { id: 'jobs', label: 'Jobs' },
  { id: 'backups', label: 'Backups' },
  { id: 'picks', label: 'Picks' },
  { id: 'profile', label: 'Profile' },
  { id: 'players', label: 'Players' },
  { id: 'ai-usage', label: 'AI usage' },
  { id: 'devices', label: 'Devices and API' },
  { id: 'big-screen', label: 'Big screen' },
  { id: 'linux', label: 'Linux and looks' },
  { id: 'about', label: 'About' },
] as const

/** Only on the Windows computer UpNext runs on, before Linux and looks. */
const WINDOWS_SECTION = { id: 'windows', label: 'Windows' } as const

/** With two or more players: who it's shared with, in the "You" group. */
const SHARING_SECTION = { id: 'sharing', label: 'Sharing' } as const

type SectionLink = (typeof SECTIONS)[number] | typeof WINDOWS_SECTION | typeof SHARING_SECTION
type SectionId = SectionLink['id']

/** With two or more players, Settings is split: the current
 *  player's own sections first, then the household's. */
const YOURS: readonly SectionId[] = ['players', 'accounts', 'sharing', 'picks', 'profile', 'ai-usage', 'linux']

/** The sections on this page: Windows only on the Windows computer UpNext
 *  runs on; with two or more players, Sharing too, and yours first. */
function sectionsFor(windows: boolean, multi = false): readonly SectionLink[] {
  let all: SectionLink[] = [...SECTIONS]
  if (windows) {
    const at = all.findIndex((sec) => sec.id === 'linux')
    all = [...all.slice(0, at), WINDOWS_SECTION, ...all.slice(at)]
  }
  if (!multi) return all
  all = [...all, SHARING_SECTION]
  const yours = YOURS.map((id) => all.find((sec) => sec.id === id)).filter((sec): sec is SectionLink => !!sec)
  return [...yours, ...all.filter((sec) => !YOURS.includes(sec.id))]
}

/** A group's heading in a split Settings: "You" or the household's. */
function GroupHeading({ title, line }: { title: string; line: string }) {
  return (
    <div className="-mb-4 flex flex-col gap-1">
      <p className="m-0 font-mono text-[11.5px] tracking-[0.1em] text-accent uppercase">{title}</p>
      <p className="m-0 text-[14px] text-muted">{line}</p>
    </div>
  )
}

export function SettingsPage() {
  const location = useLocation()
  const settings = useSettings({ staleTime: 10_000 })
  const save = useSettingsSave()
  const s = settings.data
  // The tray's settings are the computer UpNext runs on's own (403 from another device).
  const windows = s?.os === 'windows' && !!s.listen.this_computer
  const { multi, current } = usePlayers()
  const sections = useMemo(() => sectionsFor(windows, multi), [windows, multi])
  const active = useActiveSection(sections, !!s)

  useSignInReturn({ steam: 'Your SteamID is saved', gog: 'Playtime comes from GOG on the next sync', gogErrors: true })

  // Jump to #jobs, #backups or #ai-usage once the page has content.
  const loaded = !!s
  useEffect(() => {
    if (!loaded || !location.hash) return
    document.getElementById(location.hash.slice(1))?.scrollIntoView({ block: 'start' })
  }, [loaded, location.key, location.hash])

  const jump = (id: string) => (e: React.MouseEvent) => {
    e.preventDefault()
    const el = document.getElementById(id)
    if (!el) return
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    el.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'start' })
    window.history.replaceState(window.history.state, '', `${location.pathname}${location.search}#${id}`)
  }

  return (
    <div className="mx-auto grid max-w-[1440px] grid-cols-1 gap-x-14 gap-y-4 px-4 pt-8 pb-24 sm:px-10 xl:grid-cols-[180px_minmax(0,1fr)]">
      <nav aria-label="Settings sections" className="flex flex-wrap gap-x-5 self-start text-[15px] xl:sticky xl:top-[calc(var(--titlebar-h)+6rem)] xl:flex-col xl:gap-0.5">
        {sections.map((sec) => {
          const on = active === sec.id
          return (
            <a
              key={sec.id}
              href={`#${sec.id}`}
              onClick={jump(sec.id)}
              aria-current={on ? 'true' : undefined}
              className={cx('flex h-10 items-center gap-2.5 rounded-sm', on ? 'font-semibold text-ink' : 'text-muted hover:text-ink xl:pl-4')}
            >
              {on && <span aria-hidden className="h-1.5 w-1.5 rounded-full bg-accent" />}
              {sec.label}
            </a>
          )
        })}
      </nav>

      <div className="flex min-w-0 flex-col gap-10">
        {settings.isError ? (
          <p className="text-sm text-bad">Couldn't load settings: {settings.error.message}</p>
        ) : !s ? (
          <div className="space-y-4" aria-busy="true">
            <Skeleton className="h-6 w-40" />
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-2/3" />
          </div>
        ) : (
          <>
            <PlayingAs />
            {sections.map((sec, i) => {
              const body = {
                setup: <SetupSection thisComputer={!!s.listen.this_computer} />,
                accounts: <AccountsSection s={s} save={save} />,
                data: <LibraryDataSection />,
                jobs: <JobsSection s={s} />,
                backups: <BackupsSection />,
                picks: <PicksSection s={s} save={save} />,
                profile: <ProfileSection />,
                players: <PlayersSection thisComputer={!!s.listen.this_computer} />,
                sharing: <SharingSection />,
                'ai-usage': <AiUsageSection />,
                devices: <DevicesSection s={s} save={save} />,
                'big-screen': <BigScreenSection s={s} />,
                windows: <WindowsSection s={s} save={save} />,
                linux: <LooksSection s={s} save={save} />,
                about: <AboutSection />,
              }[sec.id]
              const first = multi && i === 0
              const household = multi && !YOURS.includes(sec.id) && (i === 0 || YOURS.includes(sections[i - 1].id))
              return (
                <Fragment key={sec.id}>
                  {first && <GroupHeading title={current ? `You: ${current.name}` : 'You'} line="Your own settings, accounts and who they’re shared with." />}
                  {household && <GroupHeading title="Everyone" line="Everything below is the same for everyone on this computer." />}
                  {body}
                </Fragment>
              )
            })}
          </>
        )}
      </div>
    </div>
  )
}

/** The section whose top has most recently scrolled past the top bar. */
function useActiveSection(sections: readonly SectionLink[], ready: boolean): string {
  const [active, setActive] = useState<string>(sections[0].id)
  useEffect(() => {
    if (!ready) return
    let timer = 0
    const update = () => {
      timer = 0
      const doc = document.documentElement
      const atBottom = doc.scrollHeight > window.innerHeight + 4 && window.innerHeight + window.scrollY >= doc.scrollHeight - 4
      let current: string = sections[0].id
      for (const sec of sections) {
        const el = document.getElementById(sec.id)
        if (el && el.getBoundingClientRect().top <= 140) current = sec.id
      }
      if (atBottom) current = sections[sections.length - 1].id
      setActive(current)
    }
    const onScroll = () => {
      if (!timer) timer = window.setTimeout(update, 60)
    }
    timer = window.setTimeout(update, 0)
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll)
    return () => {
      if (timer) window.clearTimeout(timer)
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onScroll)
    }
  }, [sections, ready])
  return active
}
