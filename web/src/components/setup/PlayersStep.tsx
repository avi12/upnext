import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Plus } from 'lucide-react'
import { api, ApiError } from '../../api'
import { remember, usePlayers } from '../../lib/players'
import { TextButton } from '../settings/parts'
import { StepButtons, StepHead, type StepProps } from './parts'
import { keys } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** 12. Who plays here?: the name of whoever runs setup, offered
 *  from their Steam name or this computer's user name while theirs is the
 *  made-up "Player 1", then anyone else's. Every name is
 *  optional; most households add people later, from Settings › Players. */
export function PlayersStep({ setup, next, back }: StepProps) {
  const qc = useQueryClient()
  const { players, current, bound } = usePlayers()
  const you = setup.you
  const [own, setOwn] = useState(() => (you.placeholder ? (you.suggested ?? '') : you.name))
  const [names, setNames] = useState<string[]>([''])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const typed = names.map((n) => n.trim()).filter(Boolean)
  const others = players.filter((p) => p.id !== current?.id)

  const mine = own.trim()
  // A device bound to one player can't rename anyone (Settings › Players
  // on the computer can), so it isn't asked.
  const renaming = !bound && mine !== '' && mine !== (current?.name ?? you.name)

  const save = async () => {
    setBusy(true)
    setError(null)
    // Your own name first, then the others one at a time, so a name that's
    // taken stops there and the ones already saved stay saved.
    const left = [...typed]
    let failed = false
    try {
      if (renaming) await api.changePlayer(you.id, { name: mine })
      while (left.length > 0) {
        await api.addPlayer(left[0])
        left.shift()
      }
    } catch (e) {
      failed = true
      setError(errText(e))
    } finally {
      setBusy(false)
      setNames(left.length > 0 ? left : [''])
      // Whoever runs setup stays the one playing in this browser, so
      // "Who's playing?" doesn't open over the wizard.
      if (current) remember(current.id)
      await qc.invalidateQueries({ queryKey: keys.players })
      await qc.invalidateQueries({ queryKey: ['setup'] })
    }
    if (!failed) next()
  }

  return (
    <section aria-labelledby="step-players" className="flex flex-col gap-6">
      <StepHead id="step-players" title="Who plays here?">
        <p className="m-0">
          Anyone else who plays on this computer gets their own picks, history and taste profile, and sees the games you share
          with them, without your hours. The first time someone chooses them, UpNext asks them a few short questions of their own.
        </p>
        <p className="m-0">Just you? Leave the others empty: Settings › Players adds people any time.</p>
      </StepHead>
      {!bound && (
        <label className="flex max-w-[420px] flex-col gap-1 text-[13.5px] text-dim">
          Your name
          <input
            className="field text-sm"
            value={own}
            maxLength={40}
            placeholder={current?.name ?? you.name}
            onChange={(e) => setOwn(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !busy) void save()
            }}
          />
          <span className="text-[13px]">
            What “Who’s playing?” shows; it isn’t sent anywhere.{you.placeholder ? ` Left empty, you stay ${you.name}.` : ''}
          </span>
        </label>
      )}
      <h2 className="m-0 text-[15.5px] font-bold text-ink">Who else plays here?</h2>
      {others.length > 0 && (
        <p className="m-0 text-[15px] text-muted">
          Already here: <span className="text-ink">{others.map((p) => p.name).join(', ')}</span>
        </p>
      )}
      <div className="flex max-w-[420px] flex-col gap-2.5">
        {names.map((n, i) => (
          <label key={i} className="flex flex-col gap-1 text-[13.5px] text-dim">
            {i === 0 ? 'Their name' : 'And'}
            <input
              className="field text-sm"
              value={n}
              maxLength={40}
              onChange={(e) => setNames((cur) => cur.map((x, j) => (j === i ? e.target.value : x)))}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && typed.length > 0 && !busy) void save()
              }}
            />
          </label>
        ))}
        <div>
          <TextButton onClick={() => setNames((cur) => [...cur, ''])} disabled={names[names.length - 1].trim() === ''}>
            <Plus aria-hidden="true" className="mr-1 inline h-3.5 w-3.5" />
            Someone else
          </TextButton>
        </div>
        {error && (
          <p role="alert" className="m-0 text-[14px] text-bad">
            {error}
          </p>
        )}
      </div>
      <StepButtons
        onNext={() => (typed.length > 0 || renaming ? void save() : next())}
        next={typed.length === 0 ? 'Next' : typed.length === 1 ? `Add ${typed[0]}` : `Add ${typed.length} players`}
        disabled={busy}
        onBack={back}
        onSkip={next}
      />
    </section>
  )
}
