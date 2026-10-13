import { useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GameView, OverrideStatus } from '../../types'
import { useToast } from '../useToast'
import { useOfferNextInSeries } from '../nextInSeries'
import type { Reasons } from '../StatusMenu'

/** Everything that lists games or counts them. */
export function invalidateLibrary(qc: QueryClient) {
  for (const key of ['games', 'stats', 'profile', 'history', 'collections', 'play-later', 'later']) {
    qc.invalidateQueries({ queryKey: [key] })
  }
}

function message(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

export const OVERRIDE_TOAST: Record<OverrideStatus, string> = {
  done: 'Marked as beaten',
  shelved: 'Shelved: you mean to finish it',
  abandoned: 'Marked as abandoned',
  still_playing: 'Marked as still playing',
  burnt_out: 'Burnt out: out of picks for a while',
  hidden: "Marked not for you: it won't be recommended",
}

/** Set or clear the user's status, with why it was put down when given, and
 *  an undo toast. */
export function useSetStatus(onUpdated: (g: GameView) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  const offerNext = useOfferNextInSeries()
  const m = useMutation({
    mutationFn: ({
      id,
      status,
      reasons,
    }: {
      id: number
      status: OverrideStatus | null
      previous: OverrideStatus | null
      reasons?: Reasons
      quiet?: boolean
    }) => api.setOverride(id, status, reasons),
    onSuccess: (g, vars) => {
      qc.setQueryData(['game', g.id], g)
      invalidateLibrary(qc)
      onUpdated(g)
      if (vars.quiet) return
      const title = vars.status ? OVERRIDE_TOAST[vars.status] : 'Status cleared'
      toast.success(title, g.name, {
        label: 'Undo',
        onClick: () => m.mutate({ id: g.id, status: vars.previous, previous: vars.status, quiet: true }),
      })
      offerNext(g.id, vars.status)
    },
    onError: (e) => toast.error("Couldn't change the status", message(e)),
  })
  return m
}

/** Whether the game can be played endlessly, set on its sheet; null lets UpNext infer it again. */
export function useSetEnding(onUpdated: (g: GameView) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: ({ id, noEnding }: { id: number; noEnding: boolean | null }) => api.setNoEnding(id, noEnding),
    onSuccess: (g) => {
      qc.setQueryData(['game', g.id], g)
      invalidateLibrary(qc)
      onUpdated(g)
    },
    onError: (e) => toast.error("Couldn't change that", message(e)),
  })
}

/** Save for later / remove from later. */
export function useLater(onUpdated: (g: GameView) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: ({ id, save }: { id: number; save: boolean }) => (save ? api.saveForLater(id) : api.removeFromLater(id)),
    onSuccess: (g, vars) => {
      qc.setQueryData(['game', g.id], g)
      invalidateLibrary(qc)
      onUpdated(g)
      toast.success(vars.save ? 'Saved for later' : 'Removed from later', g.name)
    },
    onError: (e) => toast.error("Couldn't update your later list", message(e)),
  })
}

/** Detach one store copy into its own title. */
export function useSplit(onUpdated: (g: GameView) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: ({ titleId, gameId }: { titleId: number; gameId: number }) => api.splitCopy(titleId, gameId),
    onSuccess: (res) => {
      qc.setQueryData(['game', res.title.id], res.title)
      qc.setQueryData(['game', res.detached.id], res.detached)
      invalidateLibrary(qc)
      toast.success(`${res.detached.name} is its own title now`)
      onUpdated(res.title)
    },
    onError: (e) => toast.error("Couldn't split that copy off", message(e)),
  })
}

/** Fold another title into this one. */
export function useMerge(game: GameView, onMerged: (g: GameView) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: (other: GameView) => api.mergeTitles([game.id, other.id]),
    onSuccess: (merged, other) => {
      qc.setQueryData(['game', merged.id], merged)
      invalidateLibrary(qc)
      toast.success(`Folded ${other.name} into ${merged.name}`)
      onMerged(merged)
    },
    onError: (e) => toast.error("Couldn't merge those", message(e)),
  })
}
