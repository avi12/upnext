import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api } from '../../api'
import type { GameView } from '../../types'
import { hoursToText } from '../../lib/format'
import { Cover } from '../Cover'
import { PLAY_LATER_KEY } from './useGameActions'
import { savedText } from '../../lib/gameText'

/** The "Play later" row on the Pick page: up to six saved games. Hidden when empty. */
export function PlayLaterShelf({ onOpen }: { onOpen: (g: GameView) => void }) {
  const { data } = useQuery({ queryKey: PLAY_LATER_KEY, queryFn: api.playLater, staleTime: 30_000 })
  if (!data || data.length === 0) return null
  const shown = data.slice(0, 6)

  return (
    <section aria-labelledby="later-heading" className="flex flex-col gap-3.5 pt-9">
      <div className="flex items-baseline justify-between gap-4">
        <div className="flex items-baseline gap-3">
          <h2 id="later-heading" className="font-display text-lg font-bold">
            Play later
          </h2>
          <span className="mono">{data.length} saved</span>
        </div>
        <Link to="/lists" className="text-[14.5px] text-muted underline decoration-underline underline-offset-4 hover:text-ink">
          See all
        </Link>
      </div>
      <ul className="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-6">
        {shown.map((g) => {
          const hours = hoursToText(g.hltb_main)
          return (
            <li key={g.id} className="min-w-0">
              <button
                type="button"
                onClick={() => onOpen(g)}
                className="group flex w-full min-w-0 items-center gap-3 rounded-[8px] text-left"
              >
                <Cover game={g} shape="portrait" className="h-16 w-12 shrink-0" titleSize="text-[8px]" />
                <span className="flex min-w-0 flex-col gap-[3px]">
                  <span className="truncate text-[14.5px] font-semibold text-ink group-hover:underline group-hover:decoration-underline group-hover:underline-offset-4">
                    {g.name}
                  </span>
                  <span className="truncate font-mono text-[10.5px] text-dim">
                    {hours ? `${hours} · ` : ''}
                    {savedText(g.saved_at)}
                  </span>
                </span>
              </button>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
