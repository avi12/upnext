import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '../../api'
import type { PickBatch, Suggestion, WishState } from '../../types'
import { Cover } from '../Cover'
import { useToast } from '../useToast'
import { LAST_BATCH_KEY } from './useGameActions'
import { wishCover, wishMeta } from './text'

const link = 'btn-quiet text-ink underline decoration-underline underline-offset-4 disabled:opacity-50'

/** A game you don't own that the AI suggested with the lineup:
 *  "Not in your library, but right down your alley", with Add to your
 *  wishlist and Not interested. An answer shows in place, with Undo. */
export function SuggestionCard({ batchId, suggestion }: { batchId: string; suggestion: Suggestion }) {
  const qc = useQueryClient()
  const toast = useToast()
  const g = suggestion.game
  const answer = useMutation({
    mutationFn: (state: Exclude<WishState, 'in_library'>) => api.wishlistSet(g.id, state),
    onSuccess: (_, state) => {
      qc.setQueryData<PickBatch | null>(LAST_BATCH_KEY, (b) =>
        b && b.batch_id === batchId && b.suggestion ? { ...b, suggestion: { ...b.suggestion, game: { ...b.suggestion.game, state } } } : b,
      )
      qc.invalidateQueries({ queryKey: ['wishlist'] })
      qc.invalidateQueries({ queryKey: ['history', 'list'] })
    },
    onError: (e) => toast.error("Couldn't answer that", e instanceof Error ? e.message : String(e)),
  })
  if (g.state === 'owned_elsewhere' || g.state === 'in_library') return null
  const meta = wishMeta(g)
  return (
    <section aria-label="A game you don't own" className="fade-up mt-6 flex flex-wrap items-center gap-x-6 gap-y-3 rounded-xl bg-raised px-5 py-4">
      <Cover game={wishCover(g)} className="w-14 shrink-0 rounded-[8px]" titleSize="text-[9px]" />
      <div className="flex min-w-0 flex-1 basis-64 flex-col gap-1">
        <p className="m-0 text-[15.5px]">
          Not in your library, but right down your alley: <span className="font-bold">{g.name}</span>
        </p>
        <p className="m-0 max-w-2xl text-[14.5px] leading-[1.5] text-muted">{suggestion.reason}</p>
        {meta && <div className="mono">{meta}</div>}
      </div>
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[14.5px]" aria-live="polite">
        {g.state === 'suggested' && (
          <>
            <button type="button" className="btn-outline h-10" disabled={answer.isPending} onClick={() => answer.mutate('wishlisted')}>
              Add to your wishlist
            </button>
            <button type="button" className={link} disabled={answer.isPending} onClick={() => answer.mutate('dismissed')}>
              Not interested
            </button>
          </>
        )}
        {(g.state === 'wishlisted' || g.state === 'dismissed') && (
          <>
            <span className="text-muted">{g.state === 'wishlisted' ? "On your wishlist." : "Not interested: it won't be suggested again."}</span>
            <button type="button" className={link} disabled={answer.isPending} onClick={() => answer.mutate('suggested')}>
              Undo
            </button>
          </>
        )}
      </div>
    </section>
  )
}
