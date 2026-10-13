import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useSearchParams } from 'react-router'
import { api } from '../api'
import type { SetupBackgroundAsk, SetupChange, SetupInfo } from '../types'
import { cx } from '../lib/format'
import { useThemeSync } from '../lib/theme'
import { PlayMark } from '../components/Shell'
import { Skeleton } from '../components/Skeleton'
import { useToast } from '../components/useToast'
import { useSignInReturn } from '../lib/useSignInReturn'
import { TextButton } from '../components/settings/parts'
import { useSettingsSave } from '../components/settings/useSettingsSave'
import { backgroundAsk, STEPS, type StepId } from '../lib/setup'
import { setBigScreen } from '../lib/bigscreen'
import { whenFocused } from '../lib/gamepad/focus'
import type { StepProps } from '../components/setup/parts'
import { ExtrasStep, SteamStep, StoresStep, WelcomeStep, WhereStep } from '../components/setup/StoreSteps'
import { AiJobsStep, AiStep } from '../components/setup/AiSteps'
import { MoreStoresStep } from '../components/setup/MoreStoresStep'
import { DetailsStep, DoneStep, SyncStep, TasteStep } from '../components/setup/JobSteps'
import { PlayersStep } from '../components/setup/PlayersStep'
import { keys, useSettings } from '../lib/queries'

const VIEWS: Record<StepId, (p: StepProps) => React.ReactNode> = {
  welcome: WelcomeStep,
  where: WhereStep,
  steam: SteamStep,
  stores: StoresStep,
  more_stores: MoreStoresStep,
  extras: ExtrasStep,
  ai: AiStep,
  ai_jobs: AiJobsStep,
  sync: SyncStep,
  details: DetailsStep,
  taste: TasteStep,
  players: PlayersStep,
  done: DoneStep,
}

const headingId = (id: StepId) => `step-${id.replace('_', '-')}`

/** The first-run wizard: every page comes here until the library
 *  has games or setup is finished or skipped, and Settings › Accounts runs it
 *  again. Each step reuses the Settings page's own rows. */
export function SetupPage() {
  useThemeSync()
  const qc = useQueryClient()
  const toast = useToast()
  const navigate = useNavigate()
  const [params] = useSearchParams()
  const setup = useQuery({
    queryKey: ['setup'],
    queryFn: api.setup,
    // While a job runs, its progress and the library's counts move; while a
    // container installs Claude Code; and while the background work waits
    // to start something (a sync, Fetch details, the shared data once the
    // first sync brought games), so the step shows the change.
    refetchInterval: (q) => {
      const d = q.state.data
      const bg = d?.background
      const seed = bg?.seed.state === 'applying' || (bg?.seed.state === 'waiting' && !!d?.library.titles)
      return d?.library.job || d?.ai.claude_install?.installing || bg?.sync_waiting || bg?.details_waiting || seed ? 2000 : false
    },
  })
  const settings = useSettings({ staleTime: 10_000 })
  const save = useSettingsSave()
  const [step, setStep] = useState<StepId | null>(null)

  // A saved setting (a key, a sign-in) changes what setup reports.
  const settingsAt = settings.dataUpdatedAt
  useEffect(() => {
    if (settingsAt) void qc.invalidateQueries({ queryKey: ['setup'] })
  }, [settingsAt, qc])

  // Start where it was left, or at the beginning when run again.
  const data = setup.data
  useEffect(() => {
    if (step || !data) return
    const saved = STEPS.find((x) => x.id === data.setup.step)?.id
    // Once, when setup's state first comes: the step is the page's own from then on.
    // eslint-disable-next-line react/set-state-in-effect
    setStep(params.has('again') || data.setup.setup !== 'open' ? 'welcome' : (saved ?? 'welcome'))
  }, [data, step, params])

  useSignInReturn({ steam: 'Your SteamID is saved', gog: 'Its games come in with the first sync' })

  // A first run whose first input is a gamepad goes to big screen's own
  // setup: someone holding a controller has no keyboard
  // to type into these steps with. Leaving big screen comes back here.
  const firstRun = data?.setup.setup === 'open' && !data.library.titles
  useEffect(() => {
    if (!firstRun) return
    const onPad = () => setBigScreen(true)
    return whenFocused(() => {
      window.addEventListener('gamepadconnected', onPad)
      return () => window.removeEventListener('gamepadconnected', onPad)
    })
  }, [firstRun])

  // Focus the new step's heading, so a screen reader starts there (not
  // on the first one: the page has just loaded).
  const first = useRef(true)
  useEffect(() => {
    if (!step) return
    if (first.current) {
      first.current = false
      return
    }
    window.scrollTo({ top: 0 })
    document.getElementById(headingId(step))?.focus({ preventScroll: true })
  }, [step])

  const record = useMutation({
    mutationFn: (change: SetupChange) => api.updateSetup(change),
    onSuccess: (state) => qc.setQueryData(keys.setupState, state),
  })
  // The sync and Fetch details run in the background as the person goes on.
  const background = useMutation({
    mutationFn: (ask: SetupBackgroundAsk) => api.setupBackground(ask),
    onSuccess: (bg) => {
      qc.setQueryData<SetupInfo>(['setup'], (old) => old && { ...old, background: bg })
      void qc.invalidateQueries({ queryKey: ['setup'] })
      void qc.invalidateQueries({ queryKey: keys.jobs })
    },
    onError: (e) => toast.error("Couldn't start the sync", e instanceof Error ? e.message : String(e)),
  })

  // The shared data is applied on the server when the first sync ends,
  // whichever step is open by then: say so once, wherever the person is.
  const seed = data?.background.seed
  const seedWas = useRef(seed?.state)
  useEffect(() => {
    const was = seedWas.current
    seedWas.current = seed?.state
    if (!seed || was === seed.state || (was !== 'waiting' && was !== 'applying')) return
    if (seed.state === 'applied') {
      for (const key of ['seed', 'game', 'games', 'stats']) void qc.invalidateQueries({ queryKey: [key] })
      toast.success('UpNext’s game data applied', `${(seed.written ?? 0).toLocaleString()} of your games got ${seed.written === 1 ? 'a summary' : 'summaries'} from it.`)
    } else if (seed.state === 'failed') {
      toast.error("Couldn't apply UpNext's game data", `${seed.error ?? 'no reason given'}. Settings › Library data offers it again.`)
    }
  }, [seed, qc, toast])

  const go = (id: StepId) => {
    const ask = step && backgroundAsk(step, id)
    if (ask) background.mutate(ask)
    setStep(id)
    record.mutate(id === 'done' ? { step: id, setup: 'done' } : { step: id })
  }
  const index = step ? STEPS.findIndex((x) => x.id === step) : 0
  const next = () => go(STEPS[Math.min(index + 1, STEPS.length - 1)].id)
  const back = () => go(STEPS[Math.max(index - 1, 0)].id)

  const open = data?.setup.setup === 'open'
  const leave = async () => {
    if (open) {
      try {
        qc.setQueryData(keys.setupState, await api.updateSetup({ setup: 'skipped' }))
      } catch (e) {
        toast.error("Couldn't leave setup", e instanceof Error ? e.message : String(e))
        return
      }
    }
    navigate('/')
  }

  const s = settings.data
  const View = step ? VIEWS[step] : null
  return (
    <div className="min-h-screen bg-ground text-ink">
      <header className="flex items-center gap-2.5 border-b border-line px-4 py-3 sm:px-10">
        <PlayMark className="h-6 w-6" />
        <span className="font-display text-[19px] font-bold">UpNext</span>
        <span className="text-[15px] text-dim">setup</span>
        <span className="ml-auto text-sm whitespace-nowrap text-dim lg:hidden" aria-hidden>
          {index + 1} of {STEPS.length}
        </span>
        <TextButton onClick={() => void leave()} className="ml-2 lg:ml-auto">
          {open ? 'Skip setup' : 'Leave setup'}
        </TextButton>
      </header>
      <div className="mx-auto grid max-w-[1240px] grid-cols-1 gap-x-14 gap-y-6 px-4 pt-8 pb-24 sm:px-10 lg:grid-cols-[200px_minmax(0,1fr)]">
        <nav aria-label="Setup steps" className="hidden self-start lg:sticky lg:top-[calc(var(--titlebar-h)+2rem)] lg:block">
          <ol className="m-0 flex list-none flex-col gap-0.5 p-0 text-[15px]">
            {STEPS.map((x, i) => {
              const on = x.id === step
              return (
                <li key={x.id}>
                  <button
                    type="button"
                    aria-current={on ? 'step' : undefined}
                    onClick={() => go(x.id)}
                    className={cx('flex h-10 w-full items-center gap-2.5 rounded-sm text-left', on ? 'font-semibold text-ink' : 'text-muted hover:text-ink')}
                  >
                    <span className={cx('w-5 text-right font-mono text-[12px] tabular-nums', on ? 'text-accent' : 'text-dim')}>{i + 1}</span>
                    {x.label}
                  </button>
                </li>
              )
            })}
          </ol>
        </nav>
        <main className="min-w-0">
          {setup.isError || settings.isError ? (
            <p className="text-sm text-bad" role="alert">
              Couldn't load setup: {(setup.error ?? settings.error)?.message}
            </p>
          ) : !data || !s || !View ? (
            <div className="space-y-4" aria-busy="true">
              <Skeleton className="h-8 w-64" />
              <Skeleton className="h-5 w-full max-w-[640px]" />
              <Skeleton className="h-5 w-2/3 max-w-[520px]" />
            </div>
          ) : (
            <View setup={data} s={s} save={save} go={go} next={next} back={back} />
          )}
        </main>
      </div>
    </div>
  )
}
