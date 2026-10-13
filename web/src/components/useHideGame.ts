import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '../api'
import type { GameView, HideScope } from '../types'
import { hiddenText, stillSeenText } from '../lib/players'
import { useToast } from './useToast'

/** What "Hide this game" needs to know about a game. */
export type Hideable = Pick<GameView, 'id' | 'name' | 'hidden_scope' | 'own_copy'>

/** Hide a game (a scope) or show it again (null), with Undo. Everything is
 *  fetched again after: the game leaves or comes back to every list. */
export function useHideGame(onDone?: (scope: HideScope | null) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  const apply = (id: number, scope: HideScope | null) => (scope ? api.hideGame(id, scope) : api.unhideGame(id).then(() => null))
  return useMutation({
    mutationFn: ({ game, scope }: { game: Hideable; scope: HideScope | null }) => apply(game.id, scope),
    onSuccess: (answer, { game, scope }) => {
      const before = game.hidden_scope ?? null
      toast.success(scope ? `${game.name}: ${hiddenText(scope).toLowerCase()}` : `${game.name}: no longer hidden`, answer && scope ? (stillSeenText(scope, answer) ?? undefined) : undefined, {
        label: 'Undo',
        onClick: () =>
          void apply(game.id, before)
            .then(() => qc.invalidateQueries())
            .catch((e) => toast.error(`Couldn't undo ${game.name}`, e instanceof Error ? e.message : undefined)),
      })
      onDone?.(scope)
      void qc.invalidateQueries()
    },
    onError: (e, { game }) => toast.error(`Couldn't hide ${game.name}`, e instanceof Error ? e.message : undefined),
  })
}
