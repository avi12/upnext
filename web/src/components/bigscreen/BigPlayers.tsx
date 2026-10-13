import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router'
import { api, ApiError } from '../../api'
import { setBigScreen } from '../../lib/bigscreen'
import { possessive, useChoosePlayer, usePlayers } from '../../lib/players'
import type { Player } from '../../types'
import { PinPad } from '../players/PinPad'
import { PlayerDisc } from '../players/PlayerDisc'
import { BigSheet, type MenuOption } from './BigMenu'
import { Hint } from './Glyph'
import { keys, useSetupState } from '../../lib/queries'

function message(e: unknown): string {
  return e instanceof ApiError || e instanceof Error ? e.message : String(e)
}

/** Big screen's top bar with two or more players: who's playing,
 *  opening the switcher. Nothing with one player. */
export function BigPlayerButton() {
  const { current, multi } = usePlayers()
  const [open, setOpen] = useState(false)
  if (!multi || !current) return null
  return (
    <>
      <button
        type="button"
        aria-label={`Playing as ${current.name}`}
        onClick={() => setOpen(true)}
        className="flex min-h-[3rem] shrink-0 items-center gap-[0.5rem] rounded-[0.7rem] px-[0.75rem] text-[1rem] text-muted"
      >
        <PlayerDisc player={current} size={28} />
        <span className="max-w-[10rem] truncate">{current.name}</span>
      </button>
      {open && <BigPlayers onClose={() => setOpen(false)} />}
    </>
  )
}

/** The switcher, made for a pad: each other player to play as (with the
 *  PIN pad for one who has a PIN), and the current player's own setup,
 *  which needs typing, so it leaves big screen. A device bound to one
 *  player can't switch. */
export function BigPlayers({ onClose }: { onClose: () => void }) {
  const navigate = useNavigate()
  const qc = useQueryClient()
  const { current, others, bound } = usePlayers()
  const state = useSetupState()
  const [asking, setAsking] = useState<Player | null>(null)
  const choose = useChoosePlayer(() => onClose())
  const ownSetup = useMutation({
    mutationFn: () => api.updateSetup({ player_setup: 'open', player_step: 'you' }),
    onSuccess: (now) => {
      qc.setQueryData(keys.setupState, now)
      setBigScreen(false)
      navigate('/setup/player')
    },
  })
  if (!current) return null

  if (asking) {
    return (
      <BigSheet title={`Play as ${asking.name}`} onClose={() => setAsking(null)}>
        <PinPad
          label={`${possessive(asking.name)} PIN`}
          busy={choose.isPending}
          error={choose.error ? message(choose.error) : null}
          submitLabel="Play"
          onSubmit={(pin) => choose.mutate({ id: asking.id, pin })}
        />
      </BigSheet>
    )
  }

  const needed = state.data?.player_needed === true
  const error = choose.error ?? ownSetup.error
  // The sheet stays open while a choice runs, unlike BigMenu's, so the PIN
  // pad can follow and a refusal can be read.
  const options: MenuOption[] = [
    ...(bound
      ? []
      : others.map((p) => ({
          key: `player-${p.id}`,
          label: `Play as ${p.name}`,
          hint: p.has_pin ? 'Asks for their PIN' : undefined,
          disabled: choose.isPending,
          onSelect: () => (p.has_pin ? setAsking(p) : choose.mutate({ id: p.id })),
        }))),
    {
      key: 'setup',
      label: needed ? `Set up ${current.name}` : 'Your own setup',
      hint: `${needed ? 'Not done yet. ' : ''}It needs typing, so it leaves big screen.`,
      disabled: ownSetup.isPending,
      onSelect: () => ownSetup.mutate(),
    },
  ]
  return (
    <BigSheet title="Who’s playing?" onClose={onClose}>
      <p className="m-0 flex items-center gap-[0.6rem] text-[1.1rem]">
        <PlayerDisc player={current} size={32} />
        <span>
          Playing as <b className="font-semibold">{current.name}</b>
        </span>
      </p>
      {bound && <p className="m-0 text-[1rem] text-muted">This device is {possessive(current.name)}, so it can’t switch.</p>}
      <ul className="m-0 flex list-none flex-col gap-[0.5rem] p-0">
        {options.map((o) => (
          <li key={o.key}>
            <button
              type="button"
              disabled={o.disabled}
              onClick={o.onSelect}
              className="flex min-h-[3rem] w-full flex-col items-start justify-center rounded-[0.7rem] bg-raised px-[1.1rem] py-[0.5rem] text-left text-[1.1rem] disabled:opacity-50"
            >
              <span>{o.label}</span>
              {o.hint && <span className="text-[0.85rem] text-dim">{o.hint}</span>}
            </button>
          </li>
        ))}
      </ul>
      {error && (
        <p role="alert" className="m-0 text-[1rem] text-bad">
          {message(error)}
        </p>
      )}
      <div className="flex justify-end">
        <Hint action="back">Close</Hint>
      </div>
    </BigSheet>
  )
}
