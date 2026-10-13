import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GameView, OverrideStatus, PickBatch } from '../../types'
import type { Reasons } from '../StatusMenu'
import { useToast } from '../useToast'
import { useOfferNextInSeries } from '../nextInSeries'

/** Query key for the lineup shown on the Pick page (the newest pick batch). */
export const LAST_BATCH_KEY = ['history', 'last'] as const
export const PLAY_LATER_KEY = ['play-later'] as const

/** The lineup on screen: the newest pick batch, so it survives a trip to
 *  another page or a reload. */
export function useLastBatch() {
  return useQuery({
    queryKey: LAST_BATCH_KEY,
    queryFn: async (): Promise<PickBatch | null> => (await api.history(1))[0] ?? null,
    staleTime: Infinity,
  })
}

/** A new lineup is the one on screen, and History reads again. */
export function showLineup(qc: QueryClient, b: PickBatch) {
  qc.setQueryData<PickBatch | null>(LAST_BATCH_KEY, b)
  void qc.invalidateQueries({ queryKey: ['history', 'list'] })
}

/** Write a fresh copy of a game into every cache that shows it. */
export function patchGame(qc: QueryClient, g: GameView) {
  qc.setQueryData(['game', g.id], g)
  qc.setQueryData<PickBatch | null>(LAST_BATCH_KEY, (b) =>
    b ? { ...b, picks: b.picks.map((p) => (p.owned !== false && p.id === g.id ? { ...p, game: g } : p)) } : b,
  )
  qc.setQueryData<GameView[]>(PLAY_LATER_KEY, (list) => list?.map((x) => (x.id === g.id ? g : x)))
}

/** Save for later and override marks, keeping the lineup, shelf and sheet in step. */
export function useGameActions(onUpdated?: (g: GameView) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  const offerNext = useOfferNextInSeries()
  const done = (g: GameView) => {
    patchGame(qc, g)
    onUpdated?.(g)
    qc.invalidateQueries({ queryKey: ['games'] })
    qc.invalidateQueries({ queryKey: ['collections'] })
  }
  const fail = (title: string) => (e: unknown) =>
    toast.error(title, e instanceof ApiError || e instanceof Error ? e.message : String(e))

  const later = useMutation({
    mutationFn: ({ id, save }: { id: number; save: boolean }) => (save ? api.saveForLater(id) : api.removeFromLater(id)),
    onSuccess: (g) => {
      done(g)
      qc.invalidateQueries({ queryKey: PLAY_LATER_KEY })
    },
    onError: fail("Couldn't update Play later"),
  })

  const mark = useMutation({
    mutationFn: ({ id, status, reasons }: { id: number; status: OverrideStatus | null; reasons?: Reasons }) =>
      api.setOverride(id, status, reasons),
    onSuccess: (g, vars) => {
      done(g)
      qc.invalidateQueries({ queryKey: ['stats'] })
      offerNext(g.id, vars.status)
    },
    onError: fail("Couldn't update that game"),
  })

  return { later, mark }
}
