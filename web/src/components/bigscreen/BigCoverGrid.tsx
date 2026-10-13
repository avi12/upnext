import type { ReactNode } from 'react'
import { useNavigate } from 'react-router'
import type { GameView } from '../../types'
import { Cover } from '../Cover'

/** Covers in a grid that the focus moves through like a shelf; each opens
 *  its game page. The grid remembers the cover last focused in it. */
export function BigCoverGrid({ games, label, after }: { games: GameView[]; label: string; after?: ReactNode }) {
  const navigate = useNavigate()
  return (
    <ul aria-label={label} data-focus-group={label} className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-[1.25rem] p-0">
      {games.map((g) => (
        <li key={g.id}>
          <button type="button" onClick={() => navigate(`/game/${g.id}`)} className="flex w-full flex-col gap-[0.4rem] rounded-[0.8rem] text-left">
            <Cover game={g} shape="portrait" titleSize="text-[1.05rem]" className="w-full rounded-[0.8rem]" />
            <span className="line-clamp-2 text-[0.95rem] leading-snug font-semibold">{g.name}</span>
          </button>
        </li>
      ))}
      {after && <li className="flex items-center">{after}</li>}
    </ul>
  )
}
