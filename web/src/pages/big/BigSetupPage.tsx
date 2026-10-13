import { useEffect, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router'
import { api, ApiError } from '../../api'
import type { Pairing, SetupInfo, Settings } from '../../types'
import { setBigScreen } from '../../lib/bigscreen'
import { canSwitch } from '../../lib/connect'
import { plural } from '../../lib/format'
import { steamShortcutState } from '../../lib/steamShortcut'
import { useSettingsSave } from '../../components/settings/useSettingsSave'
import { usePlayers } from '../../lib/players'
import { PinPad } from '../../components/players/PinPad'
import { keys, useSettings } from '../../lib/queries'

type Step = 'welcome' | 'libraries' | 'ai' | 'phone' | 'done'

function message(e: unknown): string {
  return e instanceof ApiError || e instanceof Error ? e.message : String(e)
}

function Head({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col gap-[0.6rem]">
      <h1 className="m-0 font-display text-[2rem] leading-tight font-bold">{title}</h1>
      {children && <div className="flex max-w-[48rem] flex-col gap-[0.5rem] text-[1.1rem] leading-[1.55] text-muted">{children}</div>}
    </div>
  )
}

function Buttons({ children }: { children: ReactNode }) {
  return <div className="flex flex-wrap gap-[0.75rem] pt-[0.5rem]">{children}</div>
}

/** A line of what was found, with a dot that says whether it was. */
function Found({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <li className="flex items-baseline gap-[0.7rem] text-[1.1rem]">
      <span aria-hidden className={`h-[0.6rem] w-[0.6rem] shrink-0 rounded-full ${ok ? 'bg-good' : 'bg-line-strong'}`} />
      <span className="min-w-0">{children}</span>
    </li>
  )
}

/** The first run with a controller: the steps that need
 *  no typing, since Steam, Heroic and Galaxy are found by themselves, then
 *  the AI and the stores' sign-ins handed to a phone or computer through a
 *  pairing whose QR code opens setup there,
 *  Add to Steam, and the big-screen tour. The full wizard is
 *  a button away, out of big screen. */
export function BigSetupPage() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [step, setStep] = useState<Step>('welcome')
  const setup = useQuery({
    queryKey: ['setup'],
    queryFn: api.setup,
    // While the first sync runs, its counts move.
    refetchInterval: (q) => (q.state.data?.library.job || q.state.data?.background.sync_waiting ? 2000 : false),
  })
  const settings = useSettings({ staleTime: 10_000 })
  const background = useMutation({
    mutationFn: api.setupBackground,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['setup'] })
      void qc.invalidateQueries({ queryKey: keys.jobs })
    },
  })
  const finish = useMutation({
    mutationFn: () => api.updateSetup({ step: 'done', setup: 'done' }),
    onSuccess: (state) => qc.setQueryData(keys.setupState, state),
  })

  const data = setup.data
  const s = settings.data
  if (setup.isError || settings.isError) {
    return (
      <p className="pt-[2rem] text-[1.1rem] text-bad" role="alert">
        Couldn't load setup: {message(setup.error ?? settings.error)}
      </p>
    )
  }
  if (!data || !s) return <p className="pt-[2rem] text-[1.1rem] text-muted">Looking at this computer…</p>

  // Setup stays open, and so does this page, until the server has it
  // done: otherwise the wizard would come back on the next start.
  const done = async (tour: boolean) => {
    try {
      await finish.mutateAsync()
    } catch {
      return
    }
    navigate('/', { replace: true, state: tour ? { bigTour: true } : null })
  }

  return (
    <div className="flex max-w-[60rem] flex-col gap-[1.5rem] pt-[2rem]">
      {step === 'welcome' && (
        <>
          <Head title="Welcome to UpNext">
            <p className="m-0">UpNext picks what to play next from the games you already own.</p>
            <p className="m-0">
              This sets it up with a controller. What needs typing, like an AI’s key or a store’s sign-in, can be finished on your phone or a
              computer.
            </p>
          </Head>
          <Buttons>
            <button type="button" className="big-btn-accent" data-focus-first="" onClick={() => setStep('libraries')}>
              Start
            </button>
            <button type="button" className="big-btn-outline" onClick={() => setBigScreen(false)}>
              The full setup, with a keyboard
            </button>
            {canSwitch() && (
              <button type="button" className="big-btn-outline" onClick={() => navigate('/connect')}>
                Use the library on another computer
              </button>
            )}
          </Buttons>
        </>
      )}
      {step === 'libraries' && (
        <Libraries
          setup={data}
          syncing={background.isPending}
          onSync={() => {
            background.mutate({ sync: 'if_changed', details: 'after_sync' })
            setStep('ai')
          }}
          onSkip={() => setStep('ai')}
          onBack={() => setStep('welcome')}
        />
      )}
      {step === 'ai' && <Ai setup={data} onPhone={() => setStep('phone')} onNext={() => setStep('done')} onBack={() => setStep('libraries')} />}
      {step === 'phone' && <Phone setup={data} s={s} onBack={() => setStep('ai')} onNext={() => setStep('done')} />}
      {step === 'done' && (
        <Done
          setup={data}
          here={!!s.listen.this_computer}
          error={finish.error ? message(finish.error) : null}
          onDone={(tour) => void done(tour)}
          onBack={() => setStep('ai')}
        />
      )}
    </div>
  )
}

function Libraries({ setup, syncing, onSync, onSkip, onBack }: { setup: SetupInfo; syncing: boolean; onSync: () => void; onSkip: () => void; onBack: () => void }) {
  const { steam, heroic, galaxy } = setup.stores
  const heroicGames = Object.values(heroic.games ?? {}).reduce((a, b) => a + b, 0)
  const any = !!steam.path || !!heroic.path || galaxy.exists
  return (
    <>
      <Head title="Your libraries">
        <p className="m-0">
          {any ? 'Found on this computer, with nothing to type:' : 'No launcher was found on this computer, so there’s nothing to read yet.'}
        </p>
      </Head>
      <ul className="m-0 flex list-none flex-col gap-[0.6rem] p-0">
        <Found ok={!!steam.path}>
          {steam.path ? `Steam${steam.local_games != null ? `, with ${plural(steam.local_games, 'game')} in its files` : ''}` : 'Steam: not found'}
        </Found>
        <Found ok={!!heroic.path}>{heroic.path ? `Heroic, with ${plural(heroicGames, 'game')} from Epic, GOG and Amazon` : 'Heroic: not found'}</Found>
        <Found ok={galaxy.exists}>{galaxy.exists ? 'GOG Galaxy' : 'GOG Galaxy: not found'}</Found>
      </ul>
      <p className="m-0 max-w-[48rem] text-[1.05rem] text-muted">
        Steam’s Web API key, GOG and Epic’s sign-ins add what these miss, such as games you haven’t installed. They need typing, so they can
        wait for your phone or computer.
      </p>
      <Buttons>
        <button type="button" className="big-btn-accent" data-focus-first="" disabled={!any || syncing} onClick={onSync}>
          Read my games
        </button>
        <button type="button" className="big-btn-outline" onClick={onSkip}>
          Not now
        </button>
        <button type="button" className="big-btn-outline" onClick={onBack}>
          Back
        </button>
      </Buttons>
    </>
  )
}

function Ai({ setup, onPhone, onNext, onBack }: { setup: SetupInfo; onPhone: () => void; onNext: () => void; onBack: () => void }) {
  const titles = setup.library.titles
  const job = setup.library.job
  return (
    <>
      <Head title="The AI">
        <p className="m-0" aria-live="polite">
          {job ? 'Reading your games now; it carries on while you go on.' : titles ? `${plural(titles, 'game')} in your library.` : ''}
        </p>
        {setup.ai.available ? (
          <p className="m-0">The AI is set up, so each pick says why it fits.</p>
        ) : (
          <p className="m-0">
            Picks work without one: UpNext ranks your games itself. An AI adds why each one fits and summaries of what players say, and
            setting one up means typing a key or signing in.
          </p>
        )}
      </Head>
      <Buttons>
        <button type="button" className="big-btn-accent" data-focus-first="" onClick={onNext}>
          {setup.ai.available ? 'Next' : 'Go on without it'}
        </button>
        {setup.environment.client_local && (
          <button type="button" className="big-btn-outline" onClick={onPhone}>
            Finish on your phone or computer
          </button>
        )}
        <button type="button" className="big-btn-outline" onClick={onBack}>
          Back
        </button>
      </Buttons>
    </>
  )
}

/** Hand the rest of setup to a phone or computer: let other devices reach
 *  UpNext if they can't, then a pairing code whose QR code opens setup. */
function Phone({ setup, s, onBack, onNext }: { setup: SetupInfo; s: Settings; onBack: () => void; onNext: () => void }) {
  const save = useSettingsSave()
  const [shown, setShown] = useState<Pairing | null>(null)
  // A code that ran out (or took five wrong tries) is gone as a used one
  // is, so the server says which it was.
  const [ended, setEnded] = useState(false)
  // With two or more players, the device is whoever is playing here's, so
  // a player with a PIN gives it first.
  const { current, multi } = usePlayers()
  const needsPin = multi && current?.has_pin === true
  const [askingPin, setAskingPin] = useState(false)
  const start = useMutation({
    mutationFn: (pin: string | null) => api.startPairing('setup', undefined, pin),
    onSuccess: (made) => {
      setShown(made)
      setEnded(false)
      setAskingPin(false)
    },
  })
  const showCode = () => (needsPin ? setAskingPin(true) : start.mutate(null))
  const reachable = s.listen.other_devices || !!shown?.address
  const code = ended ? null : (shown?.code ?? null)
  const [paired, setPaired] = useState(false)
  useEffect(() => {
    if (code == null) return
    const look = window.setInterval(() => {
      api
        .pairingOutcome(code)
        .then(({ outcome }) => {
          if (outcome === 'paired') setPaired(true)
          else if (outcome === 'ended') setEnded(true)
        })
        .catch(() => {})
    }, 3000)
    return () => window.clearInterval(look)
  }, [code])

  if (paired) {
    return (
      <>
        <Head title="Paired">
          <p className="m-0">Setup is open on the other device: carry on there, and come back here when it’s done.</p>
        </Head>
        <Buttons>
          <button type="button" className="big-btn-accent" data-focus-first="" onClick={onNext}>
            Next
          </button>
        </Buttons>
      </>
    )
  }
  return (
    <>
      <Head title="Finish on your phone or computer">
        {!reachable ? (
          <p className="m-0">
            UpNext only listens on this computer now. Let other devices on your network reach it; a phone or computer still needs this
            computer’s code before it can use UpNext.
          </p>
        ) : !shown ? (
          <p className="m-0">A code for your phone or computer: its QR code opens setup there, paired, with nothing to type.</p>
        ) : ended ? (
          <p className="m-0">That code ended before a phone or computer used it: it lasts five minutes, and five wrong tries end it. Show a new one to try again.</p>
        ) : (
          <p className="m-0">Scan the QR code with your phone’s camera, or open the address on a computer and type the code.</p>
        )}
        {s.listen.error && <p className="m-0 text-bad">{s.listen.error}</p>}
        {start.error && !askingPin && <p className="m-0 text-bad">{message(start.error)}</p>}
      </Head>
      {askingPin && current && (
        <PinPad
          label={`${current.name}’s PIN`}
          busy={start.isPending}
          error={start.error ? message(start.error) : null}
          submitLabel="Show"
          onSubmit={(pin) => start.mutate(pin)}
        />
      )}
      {shown && !ended && (
        <div className="flex flex-wrap items-center gap-[2rem]" role="status">
          {shown.qr_svg && (
            <img
              className="h-[12rem] w-[12rem] shrink-0 rounded-[0.6rem] bg-white p-[0.5rem]"
              alt={`QR code for ${shown.address}`}
              src={`data:image/svg+xml;utf8,${encodeURIComponent(shown.qr_svg)}`}
            />
          )}
          <div className="flex flex-col gap-[0.5rem]">
            {shown.address && <span className="font-mono text-[1.1rem] break-all">{shown.address}/setup</span>}
            <span className="font-mono text-[2.4rem] font-bold tracking-[0.3em]" aria-label={`Code ${shown.code.split('').join(' ')}`}>
              {shown.code}
            </span>
            <span className="text-[0.95rem] text-muted">It works for five minutes.</span>
          </div>
        </div>
      )}
      <Buttons>
        {!reachable ? (
          <button
            type="button"
            className="big-btn-accent"
            data-focus-first=""
            disabled={s.listen.forced}
            onClick={() => void save({ allow_other_devices: true }, 'Other devices can reach UpNext')}
          >
            Let other devices reach UpNext
          </button>
        ) : (
          <button type="button" className="big-btn-accent" data-focus-first="" disabled={start.isPending || askingPin} onClick={showCode}>
            {shown ? 'A new code' : 'Show the code'}
          </button>
        )}
        <button type="button" className="big-btn-outline" onClick={onNext}>
          {setup.ai.available ? 'Next' : 'Go on without it'}
        </button>
        <button type="button" className="big-btn-outline" onClick={onBack}>
          Back
        </button>
      </Buttons>
    </>
  )
}

function Done({
  setup,
  here,
  error,
  onDone,
  onBack,
}: {
  setup: SetupInfo
  here: boolean
  error: string | null
  onDone: (tour: boolean) => void
  onBack: () => void
}) {
  return (
    <>
      <Head title="Ready">
        <p className="m-0">
          {setup.library.titles
            ? `${plural(setup.library.titles, 'game')} to pick from.`
            : setup.library.job
              ? 'Your games are still coming in; Curate can start once some are here.'
              : 'Your library is empty for now: Settings can read your games later.'}
        </p>
        {error && (
          <p className="m-0 text-bad" role="alert">
            Setup couldn’t be finished: {error}
          </p>
        )}
      </Head>
      {here && <AddToSteam />}
      <Buttons>
        <button type="button" className="big-btn-accent" data-focus-first="" onClick={() => onDone(true)}>
          Show me around
        </button>
        <button type="button" className="big-btn-outline" onClick={() => onDone(false)}>
          Start picking
        </button>
        <button type="button" className="big-btn-outline" onClick={onBack}>
          Back
        </button>
      </Buttons>
    </>
  )
}

/** Add to Steam, on the computer UpNext runs on, where it can be added. */
function AddToSteam() {
  const qc = useQueryClient()
  const shortcut = useQuery({ queryKey: ['steam-shortcut'], queryFn: api.steamShortcut, staleTime: 10_000 })
  const add = useMutation({
    mutationFn: api.addToSteam,
    onSuccess: (now) => qc.setQueryData(['steam-shortcut'], now),
    onSettled: () => qc.invalidateQueries({ queryKey: ['steam-shortcut'] }),
  })
  if (!shortcut.data || shortcut.data.unavailable) return null
  const state = steamShortcutState(shortcut.data)
  return (
    <div className="flex flex-wrap items-center gap-[1rem] rounded-[0.9rem] border-[0.08rem] border-line bg-raised p-[1rem]">
      <div className="min-w-0 flex-1 basis-[20rem]">
        <div className="text-[1.15rem] font-bold">Add to Steam</div>
        <div className="text-[0.95rem] text-muted">{add.error ? message(add.error) : state.text}</div>
      </div>
      {state.added ? (
        <span className="text-[1rem] text-good">Added</span>
      ) : (
        <button type="button" className="big-btn-outline" disabled={!state.canAdd || add.isPending} onClick={() => add.mutate()}>
          {add.isPending ? 'Adding…' : 'Add to Steam'}
        </button>
      )}
    </div>
  )
}
