import { useMutation, useQueryClient, type UseQueryResult } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GameView, HiddenTitle, HideScope } from '../../types'
import { HIDDEN_ONLY_HERE, HIDE_CHOICES, hiddenLocked, hiddenText, usePlayers } from '../../lib/players'
import { dayText } from '../../lib/statuses'
import { Segmented } from '../settings/parts'
import { PinPad } from '../players/PinPad'
import { HidingHelp } from '../players/HidingHelp'
import { useHideGame } from '../useHideGame'
import { useToast } from '../useToast'

/** Lists › Hidden by me: every game you hid, with its scope,
 *  Change and Unhide. The only place they're listed. With a PIN, it asks for
 *  it first, and opens for ten minutes in this browser (a reload within
 *  them stays open), or until Lock. */
export function HiddenByMe({ hidden, onOpen }: { hidden: UseQueryResult<HiddenTitle[]>; onOpen: (g: GameView) => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const { multi, current } = usePlayers()
  const unlock = useMutation({
    mutationFn: (pin: string) => api.unlockHidden(pin),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['hidden'] })
      void qc.invalidateQueries({ queryKey: ['game'] })
    },
  })
  const lock = useMutation({
    mutationFn: () => api.lockHidden(),
    // The hidden games' pages close again too.
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['hidden'] })
      void qc.invalidateQueries({ queryKey: ['game'] })
    },
    onError: (e) => toast.error("Couldn't lock it", e instanceof Error ? e.message : undefined),
  })
  // With a PIN, the list can be closed again before its ten minutes are up.
  const lockButton = current?.has_pin && (
    <button type="button" className="btn-outline" disabled={lock.isPending} onClick={() => lock.mutate()}>
      Lock
    </button>
  )
  const hide = useHideGame()
  const open = async (t: HiddenTitle) => {
    try {
      onOpen(await api.game(t.title_id))
    } catch (e) {
      toast.error(`Couldn't open ${t.name}`, e instanceof Error ? e.message : undefined)
    }
  }
  const options = HIDE_CHOICES.map((c) => ({ value: c.scope, label: c.label }))

  if (hiddenLocked(hidden)) {
    return (
      <div className="flex max-w-[480px] flex-col items-center gap-4 py-10 text-center">
        <p className="m-0 text-[16px] text-muted">Enter your PIN to see the games you hid. Someone else may be at this computer.</p>
        <PinPad
          label="Your PIN"
          busy={unlock.isPending}
          error={unlock.error instanceof ApiError ? unlock.error.message : unlock.error ? String(unlock.error) : null}
          submitLabel="Open"
          onSubmit={(pin) => unlock.mutate(pin)}
        />
      </div>
    )
  }
  const list = hidden.data ?? []
  if (hidden.data && list.length === 0) {
    return (
      <div className="py-10">
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
          <p className="m-0 text-[15.5px] text-muted">Nothing hidden. Mark as… › Hide this game keeps a game out of sight.</p>
          {lockButton}
        </div>
        {multi && <HidingHelp />}
      </div>
    )
  }
  return (
    <>
      {list.length > 0 && (
        <div className="flex max-w-[1100px] flex-wrap items-start gap-x-5 gap-y-2 pb-2">
          <p className="m-0 min-w-0 flex-1 basis-[300px] text-[17px] leading-[1.6] text-muted">
            <span className="text-ink">
              {list.length} {list.length === 1 ? 'game' : 'games'}
            </span>{' '}
            you hid, most recent first. Only you see this list. {HIDDEN_ONLY_HERE}
          </p>
          {lockButton}
        </div>
      )}
      <ul aria-label="Hidden by me" className="m-0 max-w-[1100px] list-none p-0">
        {list.map((t) => (
          <li key={t.title_id} className="flex flex-wrap items-center gap-x-5 gap-y-2 border-b border-line py-3.5">
            <div className="min-w-0 flex-1 basis-[260px]">
              <button type="button" className="btn-quiet p-0 text-left text-[16px] font-semibold text-ink hover:underline" onClick={() => void open(t)}>
                {t.name}
              </button>
              <div className="text-[13.5px] text-dim">
                {hiddenText(t.scope)} · {dayText(t.at)}
              </div>
            </div>
            {/* Without a copy of your own, it can only be hidden from you. */}
            {multi && t.own_copy && (
              <Segmented<HideScope>
                label={`Who ${t.name} is hidden from`}
                options={options}
                value={t.scope}
                disabled={hide.isPending}
                onChange={(scope) => hide.mutate({ game: { id: t.title_id, name: t.name, hidden_scope: t.scope, own_copy: t.own_copy ?? false }, scope })}
              />
            )}
            <button
              type="button"
              className="btn-outline"
              disabled={hide.isPending}
              onClick={() => hide.mutate({ game: { id: t.title_id, name: t.name, hidden_scope: t.scope, own_copy: t.own_copy ?? false }, scope: null })}
            >
              Unhide
            </button>
          </li>
        ))}
      </ul>
      {multi && <HidingHelp />}
    </>
  )
}
