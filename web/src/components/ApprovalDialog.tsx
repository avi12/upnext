import { useEffect, useId, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import type { WaitingDevice } from '../types'
import { deviceKindText, remoteName } from '../lib/connect'
import { useBigScreen } from '../lib/bigscreen'
import { focusFirst } from '../lib/focus/engine'
import { useBack } from '../lib/focus/back'
import { cx } from '../lib/format'
import { pinOk, usePlayers } from '../lib/players'
import { PinField } from './players/PinField'
import { useInertOutside } from './useInertOutside'
import { useTabTrap } from './useTabTrap'
import { useSettings } from '../lib/queries'

/** How often the computer looks for devices asking. */
const EVERY_MS = 2000

/** How long Allow waits after a request is shown, so a click meant for the
 *  one before (answered, or gone) never allows one that took its place. */
export const ARM_MS = 1000

/** Who the device belongs to: a player's id, or null for the household's. */
type Owner = number | null

/** On the computer UpNext runs on: a device on the
 *  network asks to use UpNext, and every window and browser here shows it,
 *  with the number the device shows, until someone allows or denies it.
 *  Elsewhere (another device, a window showing another computer) it shows
 *  nothing and asks nothing. */
export function ApprovalDialog() {
  const settings = useSettings({ staleTime: 30_000 })
  const here = settings.data?.listen.this_computer === true && settings.data.listen.other_devices && remoteName() === null
  const waiting = useQuery({
    queryKey: ['pairing-requests'],
    queryFn: api.pairingRequests,
    enabled: here,
    refetchInterval: here ? EVERY_MS : false,
    refetchIntervalInBackground: true,
    retry: false,
  })
  const first = waiting.data?.requests[0]
  if (!here || !first) return null
  return <Ask key={first.id} device={first} players={waiting.data?.players ?? []} />
}

function Ask({ device, players }: { device: WaitingDevice; players: { id: number; name: string }[] }) {
  const qc = useQueryClient()
  const big = useBigScreen()
  const titleId = useId()
  const askId = useId()
  const numberId = useId()
  const layer = useRef<HTMLDivElement>(null)
  const root = useRef<HTMLDivElement>(null)
  // A phone is usually one person's: whoever is playing here, unless chosen
  // otherwise.
  const known = usePlayers()
  const playing = known.current?.id
  const [owner, setOwner] = useState<Owner>(players.find((p) => p.id === playing)?.id ?? players[0]?.id ?? null)
  // Allowed as a player with a PIN, the device acts as them without one, so it takes theirs.
  const pinned = known.players.find((p) => p.id === owner && p.has_pin)
  const [pin, setPin] = useState('')
  const answer = useMutation({
    mutationFn: (allow: boolean) => api.answerPairingRequest(device.id, allow, allow ? owner : null, allow && pinned ? pin : null),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['pairing-requests'] })
      qc.invalidateQueries({ queryKey: ['tokens'] })
    },
  })
  const deny = () => {
    if (!answer.isPending) answer.mutate(false)
  }
  // In big screen, Back (B on a pad) is Deny; the focus starts on Deny too,
  // so a stray press never allows anything.
  useBack(big, deny)
  // Elsewhere Escape is Deny, before "Who's playing?" or a page behind can
  // take it, and Tab stays in the dialog, which is modal.
  const denyRef = useRef(deny)
  useEffect(() => {
    denyRef.current = deny
  })
  useEffect(() => {
    if (big) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopPropagation()
      denyRef.current()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [big])
  useTabTrap(root)
  // The page behind is inert while the dialog shows. This comes before the
  // focus's effect, so the page is freed before the focus goes back to it.
  useInertOutside(layer)
  // The focus goes back where it was when the dialog goes (to the next
  // request's dialog first, if one takes this one's place).
  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null
    if (root.current) focusFirst(root.current)
    return () => {
      if (before?.isConnected) before.focus()
    }
  }, [])
  // The dialog is keyed by the request, so this runs for each one shown.
  const [armed, setArmed] = useState(false)
  useEffect(() => {
    const t = window.setTimeout(() => setArmed(true), ARM_MS)
    return () => window.clearTimeout(t)
  }, [])
  const error = answer.error instanceof ApiError ? answer.error.message : answer.error ? String(answer.error) : null

  return (
    <div ref={layer} className="fixed inset-x-0 top-(--titlebar-h) bottom-0 z-[120] flex items-center justify-center bg-[var(--backdrop)] p-4">
      <div
        ref={root}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={`${askId} ${numberId}`}
        data-focus-scope
        className={cx(
          'fade-up flex w-full flex-col rounded-2xl border border-line-strong bg-sheet shadow-2xl',
          big ? 'max-w-[34rem] gap-[1rem] p-[1.5rem]' : 'max-w-[440px] gap-4 p-6',
        )}
      >
        <h2 id={titleId} className={cx('m-0 font-display font-bold leading-tight', big ? 'text-[1.5rem]' : 'text-[21px]')}>
          {deviceKindText(device.kind)} called “{device.name}” wants to use UpNext
        </h2>
        <p id={askId} className={cx('m-0 text-muted', big ? 'text-[1rem]' : 'text-[14.5px]')}>
          Allow it only if it shows this number:
        </p>
        {/* A screen reader reads the spaced digits one by one; the number as
            shown would be read as a quantity ("five thousand four hundred"). */}
        <p id={numberId} className={cx('m-0 text-center font-mono font-bold tracking-[0.3em] text-ink', big ? 'text-[2.6rem]' : 'text-[40px]')}>
          <span aria-hidden="true">{device.match_code}</span>
          <span className="sr-only">{device.match_code.split('').join(' ')}</span>
        </p>
        <p className={cx('m-0 text-dim', big ? 'text-[0.85rem]' : 'text-[13px]')}>
          From {device.from}{device.via ? ` through the proxy at ${device.via}` : ''} on your network. Allowed, it can use everything here and is listed under Settings › Devices and API, where it can be
          revoked.
        </p>
        {players.length > 1 && (
          <fieldset className="m-0 flex flex-col gap-1.5 border-0 p-0">
            <legend className={cx('mb-1.5 p-0 font-semibold text-ink', big ? 'text-[1rem]' : 'text-[14px]')}>Whose device is it?</legend>
            <div className="flex flex-wrap gap-2" data-focus-group>
              {[...players.map((p) => ({ id: p.id as Owner, name: p.name })), { id: null as Owner, name: 'Everyone’s' }].map((p) => (
                <button
                  key={p.id ?? 'shared'}
                  type="button"
                  aria-pressed={owner === p.id}
                  onClick={() => {
                    setOwner(p.id)
                    setPin('')
                  }}
                  className={cx(
                    big ? 'big-chip' : 'min-h-9 rounded-full border px-3.5 text-[13.5px]',
                    owner === p.id ? 'border-accent bg-accent text-accent-ink' : 'border-line-strong text-ink hover:bg-raised',
                  )}
                >
                  {p.name}
                </button>
              ))}
            </div>
            <p className={cx('m-0 text-dim', big ? 'text-[0.85rem]' : 'text-[12.5px]')}>
              {owner === null ? 'Everyone’s: it asks who’s playing, like this computer.' : 'It acts for them alone.'}
            </p>
            {pinned && <PinField className="mt-1" name={pinned.name} value={pin} onChange={setPin} />}
          </fieldset>
        )}
        {error && (
          <p className="m-0 text-[14px] text-bad" role="alert">
            {error}
          </p>
        )}
        <div className="mt-1 flex justify-end gap-2.5">
          <button type="button" className={big ? 'big-btn-outline' : 'btn-outline'} data-focus-first="" disabled={answer.isPending} onClick={deny}>
            Deny
          </button>
          <button
            type="button"
            className={big ? 'big-btn-accent' : 'btn-accent'}
            disabled={answer.isPending || !armed || (!!pinned && !pinOk(pin))}
            onClick={() => answer.mutate(true)}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  )
}
