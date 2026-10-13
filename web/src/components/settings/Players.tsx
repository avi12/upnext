import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Lock } from 'lucide-react'
import { api, ApiError } from '../../api'
import type { Player } from '../../types'
import { askWhoIsPlaying, COLOURS, NOT_PRIVATE, pinOk, placeholderName, possessive, usePlayers } from '../../lib/players'
import { cx } from '../../lib/format'
import { useToast } from '../useToast'
import { PlayerDisc } from '../players/PlayerDisc'
import { InlineField, Row, Section, TextButton } from './parts'
import { PinField } from '../players/PinField'
import { keys } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** Settings › Players: everyone who plays here, adding, renaming,
 *  recolouring and removing them, the default player, and your own PIN. */
export function PlayersSection({ thisComputer }: { thisComputer: boolean }) {
  const qc = useQueryClient()
  const toast = useToast()
  const { players, current, bound, multi } = usePlayers()
  // Signing in with an identity provider: linking a
  // player to an account there is done on this computer.
  const oidc = useQuery({ queryKey: ['oidc'], queryFn: api.oidc, enabled: thisComputer && !bound, retry: false })
  const [open, setOpen] = useState<number | null>(null)
  const [name, setName] = useState('')
  const add = useMutation({
    mutationFn: () => api.addPlayer(name.trim()),
    onSuccess: (p) => {
      setName('')
      void qc.invalidateQueries({ queryKey: keys.players })
      toast.success(`${p.name} added`, 'They see everything shared with everyone, without anyone’s hours')
    },
    onError: (e) => toast.error("Couldn't add them", errText(e)),
  })

  return (
    <Section
      id="players"
      title="Players"
      desc={multi ? 'Each player has their own picks, marks, lists and taste profile, and one copy of every game they share.' : 'Just you so far.'}
    >
      {players.map((p) => (
        <PlayerRow
          key={p.id}
          player={p}
          me={p.id === current?.id}
          open={open === p.id}
          onToggle={() => setOpen((o) => (o === p.id ? null : p.id))}
          canChange={!bound}
          canRemove={!bound && thisComputer && players.length > 1}
          provider={thisComputer && !bound && (oidc.data?.enabled || !!p.oidc) ? (oidc.data?.name ?? 'the identity provider') : null}
        />
      ))}
      {current && <PinRow player={current} />}
      {!bound && (
        <form
          className="flex flex-wrap items-end gap-x-3 gap-y-2 border-b border-line py-3.5"
          onSubmit={(e) => {
            e.preventDefault()
            if (name.trim() && !add.isPending) add.mutate()
          }}
        >
          <label className="flex min-w-0 flex-1 basis-[220px] flex-col gap-1 text-[13.5px] text-dim">
            <span>
              <b className="text-[15.5px] font-bold text-ink">Add a player</b>
              {!multi && ' for someone else who plays here: their own picks, marks and taste profile, with your games shared'}
            </span>
            <input className="field text-sm" value={name} maxLength={40} placeholder="Their name" onChange={(e) => setName(e.target.value)} />
          </label>
          <button type="submit" className="btn-outline" disabled={!name.trim() || add.isPending}>
            {add.isPending ? 'Adding…' : 'Add'}
          </button>
        </form>
      )}
      <p className="m-0 max-w-[760px] pt-3 text-[13.5px] text-dim">{NOT_PRIVATE}</p>
    </Section>
  )
}

function PlayerRow({
  player,
  me,
  open,
  onToggle,
  canChange,
  canRemove,
  provider,
}: {
  player: Player
  me: boolean
  open: boolean
  onToggle: () => void
  canChange: boolean
  canRemove: boolean
  /** The identity provider's name, when this player can be linked to an account there. */
  provider: string | null
}) {
  const qc = useQueryClient()
  const toast = useToast()
  const [removing, setRemoving] = useState(false)
  const [pin, setPin] = useState('')
  const change = useMutation({
    mutationFn: (c: { name?: string; colour?: string; default?: true }) => api.changePlayer(player.id, c),
    onSuccess: () => void qc.invalidateQueries({ queryKey: keys.players }),
  })
  const remove = useMutation({
    mutationFn: () => api.removePlayer(player.id, player.has_pin ? pin : null),
    onSuccess: (r) => {
      setRemoving(false)
      toast.success(`Removed ${r.removed}`, `Backed up first: ${r.backup}`)
      void qc.resetQueries()
    },
    onError: (e) => toast.error(`Couldn't remove ${player.name}`, errText(e)),
  })
  const tags = [me && 'you', player.default && 'default', player.has_pin && 'PIN'].filter(Boolean).join(' · ')
  return (
    <Row
      name={
        <span className="inline-flex items-center gap-2.5">
          <PlayerDisc player={player} />
          {player.name}
          {player.has_pin && <Lock aria-hidden="true" className="h-3.5 w-3.5 text-dim" />}
        </span>
      }
      tag={tags || undefined}
      desc={
        // A name UpNext made up, such as the first player's "Player 1"
        // from before the wizard asked.
        placeholderName(player.name) && canChange
          ? `The name UpNext gave ${me ? 'you' : 'them'}, which “Who’s playing?” shows. Change gives ${me ? 'you' : 'them'} ${me ? 'your' : 'their'} own.`
          : undefined
      }
      wrap
      right={
        <>
          {!me && canChange && (
            <TextButton onClick={() => askWhoIsPlaying(player.id)} aria-label={`Play as ${player.name}`}>
              Play as
            </TextButton>
          )}
          {canChange && (
            <TextButton aria-expanded={open} aria-label={`Change ${player.name}`} onClick={onToggle}>
              {open ? 'Done' : 'Change'}
            </TextButton>
          )}
        </>
      }
    >
      {open && (
        <div className="flex max-w-[760px] flex-col gap-3 pb-1">
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
          <div className="flex flex-wrap items-center gap-2" role="radiogroup" aria-label={`${possessive(player.name)} colour`}>
            <span className="w-16 shrink-0 text-[13.5px] text-dim">Colour</span>
            {COLOURS.map((c) => (
              <button
                key={c}
                type="button"
                role="radio"
                aria-checked={player.colour === c}
                aria-label={c}
                onClick={() => change.mutate({ colour: c })}
                className={cx('h-7 w-7 rounded-full border-2', player.colour === c ? 'border-ink' : 'border-transparent')}
                style={{ background: c }}
              />
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-3">
            {!player.default && (
              <button type="button" className="btn-outline" disabled={change.isPending} onClick={() => change.mutate({ default: true })}>
                Make default
              </button>
            )}
            <span className="text-[13.5px] text-dim">
              {player.default
                ? 'The default player: scripts and devices that name nobody act for them.'
                : 'Scripts and devices that name nobody act for the default player.'}
            </span>
          </div>
          {provider && <SignInLink player={player} provider={provider} />}
          {canRemove && !removing && (
            <div>
              <button type="button" className="btn-outline text-bad" onClick={() => setRemoving(true)}>
                Remove {player.name}…
              </button>
            </div>
          )}
          {removing && (
            <div className="flex flex-col gap-2.5 rounded-sm border border-line p-3" role="group" aria-label={`Remove ${player.name}`}>
              <p className="m-0 text-[14px] text-muted">
                Remove {player.name}? UpNext backs up first, then their marks, history, taste profile, lists, store accounts with their
                games, AI accounts and paired devices go. The others lose the games and AI that came only from {possessive(player.name)}{' '}
                accounts.
              </p>
              {player.has_pin && <PinField name={player.name} value={pin} onChange={setPin} />}
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  className="btn-outline text-bad"
                  disabled={remove.isPending || (player.has_pin && !pinOk(pin))}
                  onClick={() => remove.mutate()}
                >
                  {remove.isPending ? 'Backing up and removing…' : `Remove ${player.name}`}
                </button>
                <TextButton onClick={() => setRemoving(false)}>Cancel</TextButton>
              </div>
            </div>
          )}
        </div>
      )}
    </Row>
  )
}

/** Linking a player to an account at the identity provider:
 *  while "Link the next sign-in" waits (ten minutes), a new account that
 *  signs in waits to be confirmed here as this player, with their PIN; it's
 *  never linked just for signing in first. Unlinking signs out the browsers
 *  that account signed in. */
function SignInLink({ player, provider }: { player: Player; provider: string }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [pin, setPin] = useState('')
  const done = () => {
    setPin('')
    void qc.invalidateQueries({ queryKey: keys.players })
    void qc.invalidateQueries({ queryKey: ['tokens'] })
  }
  const fail = (what: string) => (e: unknown) => toast.error(what, errText(e))
  const link = useMutation({
    mutationFn: () => api.linkOidc(player.id, player.has_pin ? pin : null),
    onSuccess: () => {
      done()
      toast.success('Waiting for the sign-in', `Sign in with ${provider} on ${possessive(player.name)} device, then confirm it here`)
    },
    onError: fail("Couldn't link"),
  })
  const confirm = useMutation({
    mutationFn: (subject: string) => api.confirmOidc(player.id, subject, player.has_pin ? pin : null),
    onSuccess: () => {
      done()
      toast.success('Linked', `That account signs in as ${player.name} now: sign in again on that device`)
    },
    onError: fail("Couldn't link"),
  })
  const disarm = useMutation({ mutationFn: () => api.disarmOidc(player.id), onSuccess: done, onError: fail("Couldn't stop it") })
  const unlink = useMutation({
    mutationFn: () => api.unlinkOidc(player.id),
    onSuccess: () => {
      done()
      toast.success('Unlinked', `Browsers that signed in as ${player.name} are signed out`)
    },
    onError: fail("Couldn't unlink"),
  })
  // The server gives these only while the link is waiting.
  const until = player.oidc_link_until ?? null
  const waiting = player.oidc_waiting ?? null
  // While it waits, look every few seconds for the account signing in.
  useEffect(() => {
    if (until == null) return
    const t = window.setInterval(() => void qc.invalidateQueries({ queryKey: keys.players }), 3000)
    return () => window.clearInterval(t)
  }, [until, qc])
  const pinField = player.has_pin && <PinField name={player.name} value={pin} onChange={setPin} />
  const pinReady = !player.has_pin || pinOk(pin)
  if (player.oidc) {
    return (
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-[13.5px] text-dim">Signs in with {provider}.</span>
        <button type="button" className="btn-outline" disabled={unlink.isPending} onClick={() => unlink.mutate()}>
          Unlink
        </button>
      </div>
    )
  }
  if (waiting) {
    const who = [waiting.name, waiting.username, waiting.email].filter((v, i, all) => v && all.indexOf(v) === i).join(', ')
    return (
      <div className="flex flex-col gap-2.5 rounded-sm border border-line p-3" role="group" aria-label={`Confirm ${possessive(player.name)} sign-in`}>
        <p className="m-0 text-[14px]">
          <b className="font-semibold">{who || 'An account'}</b>
          <span className="text-muted">
            {' '}
            (account <span className="font-mono text-[12.5px] break-all">{waiting.subject}</span>) signed in with {provider}. Is this{' '}
            {player.name}? Confirm only if it is: from then on it signs in as them.
          </span>
        </p>
        {pinField}
        <div className="flex flex-wrap items-center gap-2">
          <button type="button" className="btn-outline" disabled={confirm.isPending || !pinReady} onClick={() => confirm.mutate(waiting.subject)}>
            It's {player.name}
          </button>
          <TextButton disabled={disarm.isPending} onClick={() => disarm.mutate()}>
            Not them
          </TextButton>
        </div>
      </div>
    )
  }
  if (until != null) {
    return (
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-[13.5px] text-dim" role="status">
          Waiting for {player.name} to sign in with {provider}, until{' '}
          {new Date(until * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}. You'll confirm it here.
        </span>
        <TextButton disabled={disarm.isPending} onClick={() => disarm.mutate()}>
          Stop waiting
        </TextButton>
      </div>
    )
  }
  return (
    <div className="flex flex-wrap items-center gap-3">
      {pinField}
      <button type="button" className="btn-outline" disabled={link.isPending || !pinReady} onClick={() => link.mutate()}>
        Link the next sign-in
      </button>
      <span className="text-[13.5px] text-dim">
        So {possessive(player.name)} account at {provider} plays as them: they sign in on their device, and you confirm it's them here.
      </span>
    </div>
  )
}

/** Your PIN: set, change or remove it. Asked for when someone switches to
 *  you, when your Hidden by me list opens, and when you are removed. */
function PinRow({ player }: { player: Player }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [open, setOpen] = useState(false)
  const [now, setNow] = useState('')
  const [next, setNext] = useState('')
  // Someone who hid games from themself is offered a PIN for their list.
  const hidden = useQuery({ queryKey: ['hidden'], queryFn: api.hidden, enabled: !player.has_pin, retry: false })
  const hidSelf = (hidden.data ?? []).some((h) => h.scope !== 'others')
  const save = useMutation({
    mutationFn: (newPin: string | null) => api.setPin(player.id, player.has_pin ? now : null, newPin),
    onSuccess: (_, newPin) => {
      setOpen(false)
      setNow('')
      setNext('')
      void qc.invalidateQueries({ queryKey: keys.players })
      toast.success(newPin ? 'PIN saved' : 'PIN removed')
    },
    onError: (e) => toast.error("Couldn't change the PIN", errText(e)),
  })
  const digits = (set: (v: string) => void) => (e: React.ChangeEvent<HTMLInputElement>) => set(e.target.value.replace(/\D/g, '').slice(0, 8))
  const field = 'field w-[10rem] text-sm'
  return (
    <Row
      name="Your PIN"
      wrap
      desc={
        player.has_pin
          ? 'Asked for when someone switches to you here, and before your Hidden by me list opens.'
          : hidSelf
            ? 'You’ve hidden games from yourself: a PIN keeps your Hidden by me list to you.'
            : 'Optional: four to eight digits, asked for when someone switches to you here.'
      }
      right={
        <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          {open ? 'Cancel' : player.has_pin ? 'Change' : 'Set a PIN'}
        </TextButton>
      }
    >
      {open && (
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={(e) => {
            e.preventDefault()
            if (pinOk(next) && (!player.has_pin || pinOk(now))) save.mutate(next)
          }}
        >
          {player.has_pin && (
            <label className="flex flex-col gap-1 text-[13.5px] text-dim">
              PIN now
              <input className={field} type="password" inputMode="numeric" autoComplete="off" value={now} onChange={digits(setNow)} />
            </label>
          )}
          <label className="flex flex-col gap-1 text-[13.5px] text-dim">
            New PIN
            <input className={field} type="password" inputMode="numeric" autoComplete="off" value={next} onChange={digits(setNext)} />
          </label>
          <button type="submit" className="btn-outline" disabled={save.isPending || !pinOk(next) || (player.has_pin && !pinOk(now))}>
            Save PIN
          </button>
          {player.has_pin && (
            <button type="button" className="btn-outline" disabled={save.isPending || !pinOk(now)} onClick={() => save.mutate(null)}>
              Remove PIN
            </button>
          )}
        </form>
      )}
    </Row>
  )
}
