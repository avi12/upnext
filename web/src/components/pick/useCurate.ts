import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../api'
import type { PickBatch, PickResult, Source } from '../../types'
import { ROUTINES_KEY, chosenRoutine, type RoutineChoice } from '../../lib/routines'
import { showLineup, useLastBatch } from './useGameActions'
import { useLineup } from './useLineup'

/** One request for a lineup. */
export interface CurateRequest {
  prompt: string
  count: number
  exclude: number[]
  /** Reasons given with "Not these…" or "It's too ___", to steer away from. */
  why: string[]
  withoutAi: boolean
  /** The routine the lineup is for, or null. */
  routineId: number | null
  /** The dice's draw of one game off your radar. */
  random?: boolean
}

/** Curate, as the Pick page and big screen's Curate share it (R-WEB-3): the
 *  lineup on screen and its cards, the routine chosen, and `curate`, which
 *  asks for a lineup unless one is being asked for already. What a page
 *  does after (its own fields, its error message) it says in `onPicked`
 *  and `onFailed`. */
export function useCurate({
  hide = null,
  sources,
  input,
  onPicked,
  onFailed,
}: {
  /** A lineup the page cleared away, not shown again. */
  hide?: string | null
  sources?: Source[]
  /** "controller" for a lineup asked for on a gamepad. */
  input?: 'controller'
  onPicked?: (r: PickResult) => void
  /** With the lineup that was on screen when it failed, or null. */
  onFailed?: (e: Error, request: CurateRequest, shown: PickBatch | null) => void
}) {
  const qc = useQueryClient()
  const last = useLastBatch()
  const batch = last.data && last.data.batch_id !== hide ? last.data : null
  const lineup = useLineup(batch, sources, input)

  // The routine chip chosen; until one is, the routine that fits now.
  const [routineChoice, setRoutineChoice] = useState<RoutineChoice>(undefined)
  const routines = useQuery({ queryKey: ROUTINES_KEY, queryFn: api.routines, staleTime: 60_000 })
  const routine = chosenRoutine(routines.data?.routines ?? [], routines.data?.now, routineChoice)

  const m = useMutation({
    mutationFn: (r: CurateRequest) =>
      api.pick(r.prompt, r.count, r.exclude, sources, r.why, r.withoutAi, r.routineId, r.random ? 'random' : undefined, input),
    onSuccess: (r: PickResult) => {
      showLineup(qc, r)
      lineup.more.reset()
      onPicked?.(r)
    },
    onError: (e, r) => onFailed?.(e, r, batch),
  })

  // One request at a time: Enter is easy to press again while the AI takes
  // its seconds, and each would be another call.
  const curate = (r: CurateRequest) => {
    if (!m.isPending) m.mutate(r)
  }

  return { last, batch, lineup, routines, routine, routineChoice, setRoutineChoice, m, curate }
}
