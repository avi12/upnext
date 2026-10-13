import { useEffect, useId, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Lock, Plus } from 'lucide-react'
import { api, ApiError } from '../../api'
import type { Player, PlayersResponse } from '../../types'
import { memory, pickerNeeded, setAskEachTime, useChoosePlayer, usePlayers } from '../../lib/players'
import { cx } from '../../lib/format'
import { useBigScreen } from '../../lib/bigscreen'
import { focusFirst } from '../../lib/focus/engine'
import { useTabTrap } from '../useTabTrap'
import { PinPad } from './PinPad'
import { PlayerDisc } from './PlayerDisc'
import { keys } from '../../lib/queries'

/** "Who's playing?" over every page: when this browser hasn't chosen a
 *  player (`pickerNeeded`), when the server says nobody is chosen, and when
 *  the switcher asks for it. */
export function PlayerGate() {
  const qc = useQueryClient()
  const { data } = usePlayers()
  // Asked for by the switcher: undefined when not, null for the whole picker.
  const [asked, setAsked] = useState<number | null | undefined>(undefined)
  // Bumped once a player is chosen, so the memory is read again.
  const [, setChosen] = useState(0)
  useEffect(() => {
    const who = (e: Event) => setAsked((e as CustomEvent<number | null>).detail ?? null)
    const choose = () => {
      void qc.invalidateQueries({ queryKey: keys.players })
      setAsked(null)
    }
    window.addEventListener('upnext:who', who)
    window.addEventListener('upnext:choose-player', choose)
    return () => {
      window.removeEventListener('upnext:who', who)
      window.removeEventListener('upnext:choose-player', choose)
    }
  }, [qc])
  const needed = pickerNeeded(data, memory())
  if (!data || data.bound || data.players.length === 0 || (!needed && asked === undefined)) return null
  return (
    <WhoIsPlaying
      state={data}
      target={asked ?? null}
      canClose={!needed}
      onClose={() => setAsked(undefined)}
      onChosen={() => {
        setAsked(undefined)
        setChosen((n) => n + 1)
      }}
    />
  )
}

/** The cards, the PIN pad for a player with a PIN, Add a player and Ask
 *  each time. */
export function WhoIsPlaying({
  state,
  target,
  canClose,
  onClose,
  onChosen,
}: {
  state: PlayersResponse
  /** Straight to this player's PIN. */
  target: number | null
  canClose: boolean
  onClose: () => void
  onChosen: (p: Player) => void
}) {
  const big = useBigScreen()
  const titleId = useId()
  const root = useRef<HTMLDivElement>(null)
  const qc = useQueryClient()
  const [pinFor, setPinFor] = useState<Player | null>(() => state.players.find((p) => p.id === target && p.has_pin) ?? null)
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [ask, setAsk] = useState(() => memory().ask)
  const choose = useChoosePlayer(onChosen)
  const add = useMutation({
    mutationFn: () => api.addPlayer(name.trim()),
    onSuccess: (p) => {
      void qc.invalidateQueries({ queryKey: keys.players })
      choose.mutate({ id: p.id })
    },
  })
  useEffect(() => {
    if (root.current) focusFirst(root.current)
  }, [pinFor, adding])
  useEffect(() => {
    if (!canClose) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [canClose, onClose])
  // Tab stays in the picker: the page it covers can't be reached until
  // someone is chosen.
  useTabTrap(root)

  const pick = (p: Player) => {
    choose.reset()
    if (p.has_pin) setPinFor(p)
    else choose.mutate({ id: p.id })
  }
  const error = (e: unknown) => (e instanceof ApiError ? e.message : e ? String(e) : null)

  return (
    <div className="fixed inset-x-0 top-(--titlebar-h) bottom-0 z-[115] flex items-center justify-center overflow-y-auto bg-ground p-4">
      <div
        ref={root}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-focus-scope
        className={cx('flex w-full flex-col items-center gap-6 text-center', big ? 'max-w-[56rem]' : 'max-w-[720px]')}
      >
        <h1 id={titleId} className={cx('m-0 font-display font-[750] tracking-[-0.02em]', big ? 'text-[2.4rem]' : 'text-[32px]')}>
          {pinFor ? `${pinFor.name}’s PIN` : 'Who’s playing?'}
        </h1>
        {pinFor ? (
          <div className="flex flex-col items-center gap-4">
            <PlayerDisc player={pinFor} size={72} />
            <PinPad
              label={`${pinFor.name}’s PIN`}
              busy={choose.isPending}
              error={error(choose.error)}
              onSubmit={(pin) => choose.mutate({ id: pinFor.id, pin })}
            />
            <button type="button" className="btn-quiet text-[14px] text-muted" onClick={() => (target != null && canClose ? onClose() : setPinFor(null))}>
              {target != null && canClose ? 'Cancel' : 'Someone else'}
            </button>
          </div>
        ) : (
          <>
            <ul className="m-0 flex list-none flex-wrap justify-center gap-4 p-0" data-focus-group aria-label="Players">
              {state.players.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    onClick={() => pick(p)}
                    disabled={choose.isPending}
                    aria-current={p.id === state.current ? 'true' : undefined}
                    className={cx(
                      'flex w-36 flex-col items-center gap-3 rounded-2xl border px-3 py-5 hover:bg-raised',
                      p.id === state.current ? 'border-accent' : 'border-line-strong',
                    )}
                  >
                    <PlayerDisc player={p} size={64} />
                    <span className="flex max-w-full items-center gap-1.5 text-[16px] font-semibold text-ink">
                      <span className="truncate">{p.name}</span>
                      {p.has_pin && <Lock aria-label="Has a PIN" className="h-3.5 w-3.5 shrink-0 text-dim" />}
                    </span>
                  </button>
                </li>
              ))}
              <li>
                {adding ? (
                  <form
                    className="flex w-56 flex-col items-stretch gap-2 rounded-2xl border border-line-strong px-3 py-4"
                    onSubmit={(e) => {
                      e.preventDefault()
                      if (name.trim() && !add.isPending) add.mutate()
                    }}
                  >
                    <label className="flex flex-col gap-1 text-left text-[13.5px] text-dim">
                      Their name
                      <input className="field text-sm" value={name} maxLength={40} onChange={(e) => setName(e.target.value)} />
                    </label>
                    {add.error && (
                      <p role="alert" className="m-0 text-left text-[13px] text-bad">
                        {error(add.error)}
                      </p>
                    )}
                    <div className="flex justify-end gap-2">
                      <button type="button" className="btn-quiet text-[14px]" onClick={() => setAdding(false)}>
                        Cancel
                      </button>
                      <button type="submit" className="btn-accent" disabled={!name.trim() || add.isPending}>
                        Add
                      </button>
                    </div>
                  </form>
                ) : (
                  <button
                    type="button"
                    onClick={() => setAdding(true)}
                    className="flex w-36 flex-col items-center gap-3 rounded-2xl border border-dashed border-line-strong px-3 py-5 text-muted hover:bg-raised hover:text-ink"
                  >
                    <span className="flex h-16 w-16 items-center justify-center rounded-full border border-line-strong">
                      <Plus aria-hidden="true" className="h-7 w-7" />
                    </span>
                    <span className="text-[16px] font-semibold">Add a player</span>
                  </button>
                )}
              </li>
            </ul>
            {choose.error != null && (
              <p role="alert" className="m-0 text-[14px] text-bad">
                {error(choose.error)}
              </p>
            )}
            <label className="flex min-h-9 cursor-pointer items-center gap-2.5 text-[14px] text-muted">
              <input
                type="checkbox"
                className="h-[17px] w-[17px] accent-accent"
                checked={ask}
                onChange={(e) => {
                  setAsk(e.target.checked)
                  setAskEachTime(e.target.checked)
                }}
              />
              Ask each time UpNext opens
            </label>
            {canClose && (
              <button type="button" className="btn-outline" onClick={onClose}>
                Close
              </button>
            )}
          </>
        )}
        <p className="m-0 max-w-[34rem] text-[13px] text-dim">
          A PIN stops others switching to you here. It doesn’t hide your things from someone with this computer’s files.
        </p>
      </div>
    </div>
  )
}
