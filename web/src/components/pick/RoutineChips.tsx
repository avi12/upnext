import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api } from '../../api'
import { cx } from '../../lib/format'
import { ROUTINES_KEY, chosenRoutine, routineSummary, type RoutineChoice } from '../../lib/routines'

/** Under the request on the Pick page: the routines from the Profile as
 *  chips, the one that fits now chosen until another is. Hidden with none. */
export function RoutineChips({ choice, onChoose }: { choice: RoutineChoice; onChoose: (c: RoutineChoice) => void }) {
  const q = useQuery({ queryKey: ROUTINES_KEY, queryFn: api.routines, staleTime: 60_000 })
  const routines = q.data?.routines ?? []
  if (!routines.length) return null
  const chosen = chosenRoutine(routines, q.data?.now, choice)
  return (
    <div className="flex flex-col gap-2 pt-4 text-[14.5px]">
      <div role="group" aria-label="Routine" className="flex flex-wrap items-center gap-x-2.5 gap-y-2">
        <span className="text-dim">For:</span>
        {routines.map((r) => {
          const on = chosen?.id === r.id
          return (
            <button
              key={r.id}
              type="button"
              aria-pressed={on}
              title={routineSummary(r)}
              // Tapping the chosen routine again picks with none.
              onClick={() => onChoose(on ? null : r.id)}
              className={cx(
                'min-h-8 rounded-full border px-3 text-[13.5px] leading-tight',
                on ? 'border-accent bg-accent text-accent-ink' : 'border-line-strong text-muted hover:bg-raised hover:text-ink',
              )}
            >
              {r.name}
              {q.data?.now === r.id && <span className={cx('ml-1.5 text-[12px]', on ? 'opacity-80' : 'text-dim')}>· now</span>}
            </button>
          )
        })}
        <Link to="/profile#routines" className="text-dim underline decoration-underline underline-offset-4 hover:text-ink">
          Edit
        </Link>
      </div>
      <p className="m-0 text-[13.5px] text-dim">
        {chosen ? `Picks fit “${chosen.name}”${routineSummary(chosen, '').replace(/^(.)/, ': $1')}.` : 'No routine: picks fit only what you ask for and your profile.'}
      </p>
    </div>
  )
}
