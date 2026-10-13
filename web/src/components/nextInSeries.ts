import { useQueryClient } from '@tanstack/react-query'
import { api } from '../api'
import type { OverrideStatus } from '../types'
import { useToast } from './useToast'

/** After "Beat it": if the game is part of a series you own more of, offer
 *  to put the next one on Play later. */
export function useOfferNextInSeries() {
  const qc = useQueryClient()
  const toast = useToast()
  return (id: number, status: OverrideStatus | null) => {
    if (status !== 'done') return
    api
      .nextInSeries(id)
      .then((next) => {
        if (!next) return
        toast.info(`Next in ${next.series}: ${next.game.name}`, undefined, {
          label: 'Save for later',
          onClick: () =>
            void api.saveForLater(next.game.id).then(() => {
              qc.invalidateQueries({ queryKey: ['play-later'] })
              qc.invalidateQueries({ queryKey: ['collections'] })
              toast.success('Saved for later', next.game.name)
            }),
        })
      })
      .catch(() => {})
  }
}
