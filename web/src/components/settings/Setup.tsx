import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { SetupReset } from '../../types'
import { useToast } from '../useToast'
import { cx } from '../../lib/format'
import { usePlayers } from '../../lib/players'
import { Row, RowGrid, Section, TextButton } from './parts'
import { keys } from '../../lib/queries'

type Part = keyof SetupReset

/** What can be cleared before the wizard starts, in the order offered. */
const PARTS: { key: Part; name: string; desc: string; several?: string }[] = [
  {
    key: 'accounts',
    name: 'Store sign-ins and keys',
    desc: "Steam's key and SteamID, IGDB and RAWG, the Galaxy file, and the GOG and Epic sign-ins.",
    several: 'Yours: your SteamID, Galaxy file and GOG and Epic sign-ins. Steam’s key, IGDB and RAWG are everyone’s and stay.',
  },
  { key: 'ai', name: 'The AI choice and its key', desc: 'Which AI picks, its models, key and server address.' },
  {
    key: 'taste',
    name: 'The taste profile',
    desc: 'What you told it, the patterns it found, and the play diary.',
    several: 'Yours: what you told it, the patterns it found, and your play diary. The others keep theirs.',
  },
  {
    key: 'history',
    name: 'Statuses and history',
    desc: 'The statuses and ratings you set, why you stopped, Play later, Second chance answers, and every lineup.',
    several: 'Yours: your statuses and ratings, why you stopped, Play later, hidden games, Second chance answers and lineups. The others keep theirs.',
  },
  {
    key: 'library',
    name: 'The whole library',
    desc: 'Every game, its details and summaries, and your wishlist. The taste profile and history go with it. A sync brings the games back.',
    several:
      'Everyone’s: every game, its details and summaries, and every wishlist. Every player’s taste profile and history go with it. A sync brings the games back.',
  },
]

/** Settings › Setup: the setup wizard and the tour, findable on their own. */
export function SetupSection({ thisComputer }: { thisComputer: boolean }) {
  const { multi, bound } = usePlayers()
  return (
    <Section id="setup" title="Setup" desc="The setup wizard and the tour, whenever you want them again.">
      <RowGrid>
        {!bound && <WizardRow thisComputer={thisComputer} />}
        {multi && <OwnSetupRow />}
        <TourRow />
      </RowGrid>
    </Section>
  )
}

/** With several players, each one's own short setup, again. */
function OwnSetupRow() {
  const navigate = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const again = useMutation({
    mutationFn: () => api.updateSetup({ player_setup: 'open', player_step: 'you' }),
    onSuccess: (state) => {
      qc.setQueryData(keys.setupState, state)
      navigate('/setup/player')
    },
    onError: (e) => toast.error("Couldn't start your setup", e instanceof ApiError ? e.message : String(e)),
  })
  return (
    <Row
      name="Your own setup"
      desc="Your name and colour, the games and AI offered to you, and the taste questions."
      wrap
      right={
        <button type="button" className="btn-outline whitespace-nowrap" disabled={again.isPending} onClick={() => again.mutate()}>
          Run your setup again
        </button>
      }
    />
  )
}

/** The wizard again: on its own it only fills gaps; on this computer it can
 *  clear chosen parts first, after a backup. */
function WizardRow({ thisComputer }: { thisComputer: boolean }) {
  const [open, setOpen] = useState(false)
  return (
    <Row
      name="Setup wizard"
      desc="Stores, keys and the AI, one step at a time."
      wrap
      className={cx(open && 'lg:col-span-2')}
      right={
        <button type="button" className="btn-outline whitespace-nowrap" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          Restart the setup wizard
        </button>
      }
    >
      {open && <RestartChoice thisComputer={thisComputer} onCancel={() => setOpen(false)} />}
    </Row>
  )
}

function RestartChoice({ thisComputer, onCancel }: { thisComputer: boolean; onCancel: () => void }) {
  const navigate = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const { multi } = usePlayers()
  const [clear, setClear] = useState(false)
  const [ticked, setTicked] = useState<Set<Part>>(() => new Set())
  // The whole library takes the taste profile and the history with it.
  const on = (k: Part) => ticked.has(k) || (ticked.has('library') && (k === 'taste' || k === 'history'))
  const chosen = clear ? PARTS.filter((p) => on(p.key)) : []
  const reset = useMutation({
    mutationFn: () => api.resetSetup(Object.fromEntries(chosen.map((p) => [p.key, true]))),
    onSuccess: (done) => {
      void qc.invalidateQueries()
      toast.success('Cleared, after a backup', `${done.backup} is in Settings › Backups`)
      if (done.from_env.length > 0) toast.error('Still set outside the app', `${done.from_env.join(', ')}: from the environment or a .env file`)
      navigate('/setup?again=1')
    },
  })
  const toggle = (k: Part) =>
    setTicked((cur) => {
      const next = new Set(cur)
      if (next.has(k)) next.delete(k)
      else next.add(k)
      return next
    })

  if (!thisComputer) {
    return (
      <div className="flex flex-col gap-2.5 pt-3">
        <p className="m-0 max-w-[760px] text-[14px] text-muted">
          The wizard only fills gaps. Clearing things first is offered only on the computer UpNext runs on.
        </p>
        <div className="flex items-center gap-2">
          <Link to="/setup?again=1" className="btn-outline">
            Start the wizard
          </Link>
          <TextButton onClick={onCancel}>Cancel</TextButton>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3 pt-3">
      <fieldset className="m-0 flex flex-col gap-1 border-0 p-0">
        <legend className="mb-1 p-0 text-[14px] text-muted">What should the wizard start from?</legend>
        <label className="flex min-h-11 cursor-pointer items-center gap-2.5">
          <input type="radio" name="restart" checked={!clear} onChange={() => setClear(false)} className="m-0 h-[17px] w-[17px] accent-accent" />
          <span className="text-[14.5px]">Keep everything. The wizard only fills gaps.</span>
        </label>
        <label className="flex min-h-11 cursor-pointer items-center gap-2.5">
          <input type="radio" name="restart" checked={clear} onChange={() => setClear(true)} className="m-0 h-[17px] w-[17px] accent-accent" />
          <span className="text-[14.5px]">Clear some things first</span>
        </label>
        {clear && (
          <div className="flex flex-col gap-0.5 pl-7">
            {PARTS.map((p) => {
              const implied = !ticked.has(p.key) && on(p.key)
              return (
                <label key={p.key} className={cx('flex cursor-pointer items-start gap-2.5 py-1.5', implied && 'cursor-default')}>
                  <input
                    type="checkbox"
                    checked={on(p.key)}
                    disabled={implied}
                    onChange={() => toggle(p.key)}
                    className="m-0 mt-[3px] h-[17px] w-[17px] shrink-0 accent-accent"
                  />
                  <span className="min-w-0">
                    <span className="block text-[14.5px] font-bold">{p.name}</span>
                    <span className="block text-[13.5px] text-dim">
                      {implied ? 'Goes with the whole library.' : multi && p.several ? p.several : p.desc}
                    </span>
                  </span>
                </label>
              )
            })}
          </div>
        )}
      </fieldset>
      {chosen.length > 0 && (
        <p className="m-0 max-w-[760px] text-[14px] text-warn">
          UpNext backs up everything first, kept in Settings › Backups until you delete it. Only restoring that backup undoes this
          {on('accounts') ? ", and the GOG and Epic sign-ins aren't in backups, so you'd sign in to them again" : ''}. Keys set in a .env file stay.
        </p>
      )}
      {reset.isError && (
        <p className="m-0 text-[14px] text-bad" role="alert">
          {reset.error instanceof ApiError ? reset.error.message : String(reset.error)}
        </p>
      )}
      <div className="flex items-center gap-2">
        {chosen.length > 0 ? (
          <button type="button" className="btn-outline text-bad" disabled={reset.isPending} onClick={() => reset.mutate()}>
            {reset.isPending ? 'Backing up and clearing…' : 'Back up, clear and start'}
          </button>
        ) : (
          <button type="button" className="btn-outline" disabled={clear} onClick={() => navigate('/setup?again=1')}>
            Start the wizard
          </button>
        )}
        <TextButton onClick={onCancel}>Cancel</TextButton>
      </div>
    </div>
  )
}

/** The tour, from the Pick page. */
function TourRow() {
  return (
    <Row
      name="Tour"
      desc="Where everything is, and which words are menus. About two minutes."
      wrap
      right={
        <Link to="/?tour=1" className="btn-outline whitespace-nowrap">
          Show me around
        </Link>
      }
    />
  )
}
