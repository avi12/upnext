import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Star } from 'lucide-react'
import { api, ApiError } from '../../api'
import type { GameView } from '../../types'
import { cx } from '../../lib/format'
import { STAR_WORDS } from '../../lib/ratings'
import { useToast } from '../useToast'
import { invalidateLibrary } from './sheetActions'

/** Your own rating of a game, 1 to 5 stars, on its page.
 *  Picks read it as part of your taste profile. */
export function MyRating({ game, onUpdated }: { game: GameView; onUpdated: (g: GameView) => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [hover, setHover] = useState<number | null>(null)
  const rate = useMutation({
    mutationFn: (rating: number | null) => api.rateGame(game.id, rating),
    onSuccess: (g) => {
      qc.setQueryData(['game', g.id], g)
      invalidateLibrary(qc)
      qc.invalidateQueries({ queryKey: ['taste-sources'] })
      onUpdated(g)
    },
    onError: (e) => toast.error('Couldn’t save your rating', e instanceof ApiError ? e.message : String(e)),
  })
  const current = game.my_rating ?? null
  const shown = hover ?? current

  return (
    <div className="flex flex-wrap items-center gap-x-2.5">
      <span>Your rating</span>
      <div role="group" aria-label="Your rating" className="flex items-center" onMouseLeave={() => setHover(null)}>
        {[1, 2, 3, 4, 5].map((n) => {
          const lit = shown != null && n <= shown
          return (
            <button
              key={n}
              type="button"
              aria-pressed={current === n}
              aria-label={`${n} ${n === 1 ? 'star' : 'stars'}: ${STAR_WORDS[n - 1]}`}
              disabled={rate.isPending}
              onMouseEnter={() => setHover(n)}
              onClick={() => rate.mutate(n)}
              className="btn-quiet inline-flex min-h-10 min-w-8 items-center justify-center disabled:opacity-60"
            >
              <Star aria-hidden="true" className={cx('h-[18px] w-[18px]', lit ? 'text-accent' : 'text-dim')} fill={lit ? 'currentColor' : 'none'} />
            </button>
          )
        })}
      </div>
      {shown != null && <span className="text-ink">{STAR_WORDS[shown - 1]}</span>}
      {current != null && hover == null && (
        <button
          type="button"
          disabled={rate.isPending}
          onClick={() => rate.mutate(null)}
          className="btn-quiet min-h-10 text-[13px] text-dim underline decoration-underline underline-offset-4 hover:text-ink"
        >
          Clear
        </button>
      )}
    </div>
  )
}
