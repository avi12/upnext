import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { SeedProgress, SetupInfo } from '../../types'
import { monthYear } from '../../lib/format'
import { useToast } from '../useToast'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** The first-sync step's "Use UpNext's game data": on by default,
 *  applied by the server when the first sync ends, so the data has games to
 *  match, whichever step is open by then; the box only says whether.
 *  It isn't offered when the library had games before the sync (not a first
 *  sync), when shared data is already kept, since each sync applies that
 *  again, or when the newest release's manifest can't be read (none is
 *  published yet, or GitHub can't be reached), since there's nothing to
 *  apply; Settings › Library data offers it any time. `empty`: the library
 *  has no games yet. */
export function SharedDataOption({ seed, empty }: { seed: SeedProgress; empty: boolean }) {
  const qc = useQueryClient()
  const toast = useToast()
  const open = seed.state === 'waiting' || (seed.state === 'idle' && empty)
  const status = useQuery({ queryKey: ['seed'], queryFn: api.seed, staleTime: 30_000, enabled: open })
  // Read by the server from the newest release; without it there is nothing to offer.
  const offer = useQuery({ queryKey: ['seed-offer'], queryFn: api.seedOffer, retry: false, staleTime: Infinity, enabled: open })
  const choose = useMutation({
    mutationFn: (on: boolean) => api.setupBackground({ seed: on }),
    onSuccess: (bg) => qc.setQueryData<SetupInfo>(['setup'], (old) => old && { ...old, background: bg }),
    onError: (e) => toast.error("Couldn't change that", errText(e)),
  })

  if (seed.state === 'applying') {
    return <p className="m-0 text-[15px] text-muted">Applying UpNext’s game data…</p>
  }
  if (seed.state === 'applied') {
    return (
      <p className="m-0 text-[15px] text-good">
        UpNext’s game data is applied: {(seed.written ?? 0).toLocaleString()} of your games got summaries from it.
      </p>
    )
  }
  if (seed.state === 'failed') {
    return (
      <p className="m-0 text-[14px] text-bad" role="alert">
        UpNext’s game data wasn’t applied: {seed.error ?? 'no reason given'}. Nothing else changed; Settings › Library data offers it
        again.
      </p>
    )
  }
  if (!open || status.isPending || status.data?.kept || !offer.data) return null

  const o = offer.data
  return (
    <label className="flex max-w-[640px] cursor-pointer items-start gap-2.5">
      <input
        type="checkbox"
        className="mt-[3px] h-[17px] w-[17px] shrink-0 accent-accent"
        checked={choose.isPending ? choose.variables : seed.wanted}
        onChange={(e) => choose.mutate(e.target.checked)}
      />
      <span className="flex flex-col gap-1">
        <span className="text-[15px] text-ink">
          Use UpNext’s game data
          {` (${o.count.toLocaleString()} ${o.count === 1 ? 'game' : 'games'}, updated ${monthYear(o.date)})`}
        </span>
        <span className="text-[13.5px] text-dim">
          Ready-made summaries and tags for popular games, free for anyone to use, so your AI only has to write the rest. Added when
          the first sync finishes; Settings › Library data can stop using it.
        </span>
      </span>
    </label>
  )
}
