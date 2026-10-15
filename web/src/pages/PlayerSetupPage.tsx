import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Navigate, useNavigate } from 'react-router'
import { api, ApiError } from '../api'
import type { Player, SetupChange, SetupInfo, Settings } from '../types'
import { cx, plural } from '../lib/format'
import { useThemeSync } from '../lib/theme'
import { BACKEND_LABEL } from '../lib/ai'
import { PLAYER_STEPS, type PlayerStepId } from '../lib/setup'
import { COLOURS, possessive, usePlayers } from '../lib/players'
import { LogoMark } from '../components/Shell'
import { Skeleton } from '../components/Skeleton'
import { useToast } from '../components/useToast'
import { useSignInReturn } from '../lib/useSignInReturn'
import { InlineField, TextButton } from '../components/settings/parts'
import { useSettingsSave, type SaveFn } from '../components/settings/useSettingsSave'
import { GogRow, SteamRow } from '../components/settings/Accounts'
import { AiChooser } from '../components/settings/Picks'
import { OfferedAiAccount, OfferedStoreAccount } from '../components/settings/Sharing'
import { TasteQuestions } from '../components/questions/TasteQuestions'
import { PlayerDisc } from '../components/players/PlayerDisc'
import { StepButtons, StepHead } from '../components/setup/parts'
import { OtherUsersFolders } from '../components/setup/OtherUsersFolders'
import { keys, useSettings, useSetupState } from '../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

const headingId = (id: PlayerStepId) => `player-step-${id}`

interface PlayerStepProps {
  player: Player
  bound: boolean
  s: Settings
  save: SaveFn
  next: () => void
  back: () => void
}

/** A new player's own short setup: the first time they're chosen,
 *  after the household's wizard, every page comes here until they finish or
 *  skip it. Their name and colour, the games and AI the others offer them
 *  (each on, as offered) or their own, the taste questions, then the tour.
 *  Settings › Setup runs it again. */
export function PlayerSetupPage() {
  useThemeSync()
  const qc = useQueryClient()
  const toast = useToast()
  const navigate = useNavigate()
  const state = useSetupState({ staleTime: 0 })
  const settings = useSettings({ staleTime: 10_000 })
  const { current, bound } = usePlayers()
  const save = useSettingsSave()
  const [step, setStep] = useState<PlayerStepId | null>(null)

  const data = state.data
  useEffect(() => {
    if (step || !data) return
    // Once, when the player's setup state first comes: the step is the page's own from then on.
    // eslint-disable-next-line react/set-state-in-effect
    setStep(PLAYER_STEPS.find((x) => x.id === data.player_step)?.id ?? 'you')
  }, [data, step])

  useSignInReturn({ steam: 'Your games come in with the next sync', gog: 'Its games come in with the next sync' })

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
    onSuccess: (s) => qc.setQueryData(keys.setupState, s),
  })
  const go = (id: PlayerStepId) => {
    setStep(id)
    record.mutate({ player_step: id })
  }
  const index = step ? PLAYER_STEPS.findIndex((x) => x.id === step) : 0
  const next = () => go(PLAYER_STEPS[Math.min(index + 1, PLAYER_STEPS.length - 1)].id)
  const back = () => go(PLAYER_STEPS[Math.max(index - 1, 0)].id)

  /** End it, done or skipped, and go on to `to`. */
  const end = async (change: SetupChange, to: string) => {
    try {
      qc.setQueryData(keys.setupState, await api.updateSetup(change))
    } catch (e) {
      toast.error("Couldn't finish your setup", errText(e))
      return
    }
    navigate(to)
  }

  // Someone who doesn't need it (set up already, the only player, or the
  // household's wizard first) goes on to the app.
  if (data && !data.player_needed && data.player_step !== 'done') return <Navigate to={data.needed ? '/setup' : '/'} replace />

  const s = settings.data
  const ready = data && s && current && step
  return (
    <div className="min-h-screen bg-ground text-ink">
      <header className="flex items-center gap-2.5 border-b border-line px-4 py-3 sm:px-10">
        <LogoMark className="h-6 w-6" />
        <span className="font-display text-[19px] font-bold">UpNext</span>
        <span className="truncate text-[15px] text-dim">{current ? `${possessive(current.name)} setup` : 'setup'}</span>
        <span className="ml-auto text-sm whitespace-nowrap text-dim lg:hidden" aria-hidden>
          {index + 1} of {PLAYER_STEPS.length}
        </span>
        <TextButton onClick={() => void end({ player_setup: 'skipped' }, '/')} className="ml-2 lg:ml-auto">
          Skip setup
        </TextButton>
      </header>
      <div className="mx-auto grid max-w-[1240px] grid-cols-1 gap-x-14 gap-y-6 px-4 pt-8 pb-24 sm:px-10 lg:grid-cols-[200px_minmax(0,1fr)]">
        <nav aria-label="Setup steps" className="hidden self-start lg:sticky lg:top-[calc(var(--titlebar-h)+2rem)] lg:block">
          <ol className="m-0 flex list-none flex-col gap-0.5 p-0 text-[15px]">
            {PLAYER_STEPS.map((x, i) => {
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
          {state.isError || settings.isError ? (
            <p className="text-sm text-bad" role="alert">
              Couldn't load your setup: {(state.error ?? settings.error)?.message}
            </p>
          ) : !ready ? (
            <div className="space-y-4" aria-busy="true">
              <Skeleton className="h-8 w-64" />
              <Skeleton className="h-5 w-full max-w-[640px]" />
            </div>
          ) : step === 'you' ? (
            <YouStep player={current} bound={bound} s={s} save={save} next={next} back={back} />
          ) : step === 'games' ? (
            <GamesStep player={current} bound={bound} s={s} save={save} next={next} back={back} />
          ) : step === 'ai' ? (
            <AiStep player={current} bound={bound} s={s} save={save} next={next} back={back} />
          ) : step === 'taste' ? (
            <section aria-labelledby={headingId('taste')} className="flex flex-col gap-6">
              <StepHead id={headingId('taste')} title="Your taste">
                <p className="m-0">
                  A couple of minutes, and your first picks fit you better. Every answer saves as you give it; skip any of them.
                </p>
              </StepHead>
              <TasteQuestions onFinish={next} onEnd={next} onBack={back} onSkip={next} />
            </section>
          ) : (
            <DoneStep
              player={current}
              back={back}
              onTour={() => void end({ player_setup: 'done' }, '/?tour=1')}
              onExplore={() => void end({ player_setup: 'done', tour: 'skipped' }, '/library')}
            />
          )}
        </main>
      </div>
    </div>
  )
}

/** 1. Their name and colour. */
function YouStep({ player, bound, next }: PlayerStepProps) {
  const qc = useQueryClient()
  const change = useMutation({
    mutationFn: (c: { name?: string; colour?: string }) => api.changePlayer(player.id, c),
    onSuccess: () => void qc.invalidateQueries({ queryKey: keys.players }),
  })
  return (
    <section aria-labelledby={headingId('you')} className="flex flex-col gap-6">
      <StepHead id={headingId('you')} title={`Hi, ${player.name}`}>
        <p className="m-0">
          You have your own picks, history and taste profile here. A few short steps: your name and colour, the games and AI the
          others offer you, and a couple of questions about what you love.
        </p>
      </StepHead>
      {bound ? (
        <div className="flex items-center gap-3 text-[15px] text-muted">
          <PlayerDisc player={player} size={40} />
          This device is {possessive(player.name)}. Names and colours are changed in Settings › Players on a shared device.
        </div>
      ) : (
        <div className="flex max-w-[560px] flex-col gap-3">
          <InlineField
            label={<span className="w-16 shrink-0 text-[13.5px] text-dim">Name</span>}
            value={player.name}
            maxLength={40}
            onCommit={async (v) => {
              if (!v.trim() || v.trim() === player.name) return null
              try {
                await change.mutateAsync({ name: v.trim() })
                return null
              } catch (e) {
                return errText(e)
              }
            }}
          />
          <div className="flex flex-wrap items-center gap-2" role="radiogroup" aria-label="Your colour">
            <span className="w-16 shrink-0 text-[13.5px] text-dim">Colour</span>
            {COLOURS.map((c) => (
              <button
                key={c}
                type="button"
                role="radio"
                aria-checked={player.colour === c}
                aria-label={c}
                onClick={() => change.mutate({ colour: c })}
                className={cx('h-8 w-8 rounded-full border-2', player.colour === c ? 'border-ink' : 'border-transparent')}
                style={{ background: c }}
              />
            ))}
          </div>
          {change.isError && (
            <p role="alert" className="m-0 text-[14px] text-bad">
              {errText(change.error)}
            </p>
          )}
        </div>
      )}
      <StepButtons onNext={next} />
    </section>
  )
}

/** 2. The store accounts the others offer, each on, and their own. */
function GamesStep({ s, save, next, back }: PlayerStepProps) {
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const setup = useQuery({ queryKey: ['setup'], queryFn: api.setup, staleTime: 30_000 })
  const offered = accounts.data?.accounts.filter((a) => !a.mine) ?? []
  const mine = accounts.data?.accounts.filter((a) => a.mine) ?? []
  const env = setup.data?.environment
  return (
    <section aria-labelledby={headingId('games')} className="flex flex-col gap-6">
      <StepHead id={headingId('games')} title="Your games">
        <p className="m-0">
          The others offer you their store accounts. Each one is on, so its games are in your library; its hours count only for
          whoever plays on it, unless its owner says you play on it too. Turn off any you don’t want.
        </p>
      </StepHead>
      {accounts.isError && <p className="m-0 text-sm text-bad">Couldn't load the accounts: {errText(accounts.error)}</p>}
      <div>
        <h2 className="m-0 text-[13px] font-semibold tracking-[0.06em] text-dim uppercase">Offered to you</h2>
        {accounts.data && offered.length === 0 && (
          <p className="m-0 border-b border-line py-3 text-sm text-dim">Nobody offers you a store account yet.</p>
        )}
        {offered.map((a) => (
          <OfferedStoreAccount key={a.id} account={a} />
        ))}
      </div>
      <div>
        <h2 className="m-0 text-[13px] font-semibold tracking-[0.06em] text-dim uppercase">Your own</h2>
        <p className="m-0 max-w-[720px] pt-2 text-[14px] text-muted">
          Link your own and its games and hours are yours, offered to everyone unless you change it in Settings › Sharing.
          {mine.length > 0 && ` You have ${plural(mine.length, 'account')} already.`}
        </p>
        {setup.data?.stores && <OnThisComputer setup={setup.data} s={s} save={save} />}
        <SteamRow s={s} save={save} inWizard then="player" />
        <GogRow noWindow={env != null && !env.gog_window} back="/setup/player" />
        <p className="m-0 pt-3 text-[13.5px] text-dim">Epic, Heroic and GOG Galaxy are in Settings › Accounts.</p>
      </div>
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}

/** What this computer's Steam client and Heroic have for a new player:
 *  a Steam account signed in here that nobody has yet, to take
 *  with one click, and whose the others and the Heroic folder are. */
export function OnThisComputer({ setup, s, save }: { setup: SetupInfo; s: Settings; save: SaveFn }) {
  const steam = setup.stores.steam.local_accounts
  // Another player's account comes with whose it is and nothing else.
  const free = steam.flatMap((a) => (a.player == null && a.steam_id ? [{ ...a, steam_id: a.steam_id }] : []))
  const taken = steam.flatMap((a) => (a.player != null && a.steam_id !== s.steam_id ? [a.player] : []))
  const heroic = setup.stores.heroic
  const heroicOwner = heroic.path && heroic.owner ? heroic.owner : null
  const otherHeroic = heroic.other_users.length > 0
  if (s.steam_id && taken.length === 0 && !heroicOwner && !otherHeroic) return null
  return (
    <div className="flex flex-col gap-2 border-b border-line py-3 text-[14px]">
      {!s.steam_id &&
        free.map((a) => (
          <div key={a.steam_id} className="flex flex-wrap items-center gap-3">
            <span className="min-w-0 flex-1">
              <b className="font-semibold">{a.persona ?? a.steam_id}</b> is signed in to Steam on this computer.
            </span>
            <button type="button" className="btn-outline" onClick={() => void save({ steam_id: a.steam_id }, 'Your Steam account')}>
              That’s me
            </button>
          </div>
        ))}
      {taken.map((player, i) => (
        <p key={i} className="m-0 text-muted">
          A Steam account on this computer is {possessive(player)}.
        </p>
      ))}
      {heroicOwner && (
        <p className="m-0 text-muted">
          Heroic on this computer is {possessive(heroicOwner)}: its games are in the accounts offered to you above when they share them.
        </p>
      )}
      {otherHeroic && <OtherUsersFolders setup={setup} />}
    </div>
  )
}

/** 3. Which AI their picks use: one offered to them, or their own. */
function AiStep({ s, save, next, back }: PlayerStepProps) {
  const ai = useQuery({ queryKey: ['ai-accounts'], queryFn: api.aiAccounts })
  const [own, setOwn] = useState(false)
  const offered = ai.data?.filter((a) => !a.mine) ?? []
  const mine = ai.data?.filter((a) => a.mine) ?? []
  const inUse = ai.data?.find((a) => a.in_use)
  return (
    <section aria-labelledby={headingId('ai')} className="flex flex-col gap-6">
      <StepHead id={headingId('ai')} title="Your AI">
        <p className="m-0">
          The AI reads your library and picks what you play next, with reasons. Use one someone offers you, whose owner pays for
          it and sees what your picks cost them, or set up your own. Without one, your picks are ranked by UpNext alone.
        </p>
      </StepHead>
      {ai.isError && <p className="m-0 text-sm text-bad">Couldn't load the AI accounts: {errText(ai.error)}</p>}
      <div>
        {ai.data && offered.length === 0 && <p className="m-0 border-b border-line py-3 text-sm text-dim">Nobody offers you an AI yet.</p>}
        {offered.map((a) => (
          <OfferedAiAccount key={a.id} account={a} />
        ))}
        {mine.length > 0 && (
          <p className="m-0 py-3 text-[14px] text-muted">
            {inUse?.mine ? 'Your picks use your own AI.' : `You have your own AI too; Settings › Sharing switches to it.`}
          </p>
        )}
      </div>
      {own || (ai.data && offered.length === 0 && mine.length === 0) ? (
        <AiChooser s={s} save={save} />
      ) : (
        <div>
          <TextButton aria-expanded={own} onClick={() => setOwn(true)}>
            Set up your own instead
          </TextButton>
        </div>
      )}
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}

/** 5. What's set up, and the tour. */
function DoneStep({ player, back, onTour, onExplore }: { player: Player; back: () => void; onTour: () => void; onExplore: () => void }) {
  const setup = useQuery({ queryKey: ['setup'], queryFn: api.setup })
  const d: SetupInfo | undefined = setup.data
  return (
    <section aria-labelledby={headingId('done')} className="flex flex-col gap-6">
      <StepHead id={headingId('done')} title={`You're set up, ${player.name}`}>
        {d && (
          <ul className="m-0 flex list-disc flex-col gap-1 pl-5">
            <li>
              {d.library.titles
                ? `${plural(d.library.titles, 'game')} in your library.`
                : 'No games yet: link an account or use one the others offer you.'}
            </li>
            <li>
              {d.ai.available
                ? `Your picks use ${BACKEND_LABEL[d.ai.backend] ?? d.ai.backend}.`
                : 'No AI yet, so your picks are ranked by UpNext alone. Settings › Sharing or Picks adds one.'}
            </li>
          </ul>
        )}
        <p className="m-0">Settings › Setup runs this again, and has the tour.</p>
      </StepHead>
      <div className="flex flex-wrap items-center gap-3 border-t border-line pt-5">
        <button type="button" className="btn-accent" onClick={onTour}>
          Show me around (2 minutes)
        </button>
        <button type="button" className="btn-outline" onClick={onExplore}>
          I'll explore
        </button>
        <button type="button" className="btn-quiet text-sm underline underline-offset-4" onClick={back}>
          Back
        </button>
      </div>
    </section>
  )
}
