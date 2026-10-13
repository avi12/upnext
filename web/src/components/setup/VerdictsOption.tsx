import { useEffect } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import { useToast } from '../useToast'
import { keys } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** The first-sync step's "Backlog verdicts from other players",
 *  beside "Use UpNext's game data" and on by default as it is, since it's a
 *  download that sends nothing about the player: an install that never chose is turned on
 *  when the step shows, and the box turns it off. */
export function VerdictsOption() {
  const qc = useQueryClient()
  const toast = useToast()
  const status = useQuery({ queryKey: ['verdicts-community'], queryFn: api.communityVerdicts, staleTime: 30_000 })
  const choose = useMutation({
    mutationFn: (on: boolean) => api.updateSettings({ verdicts_receive: on }),
    onSuccess: (s) => {
      qc.setQueryData(keys.settings, s)
      qc.invalidateQueries({ queryKey: ['verdicts-community'] })
      qc.invalidateQueries({ queryKey: ['community-verdicts'] })
    },
    onError: (e) => toast.error("Couldn't change that", errText(e)),
  })
  const s = status.data
  const unchosen = s != null && !s.chosen
  const { mutate, isIdle } = choose
  useEffect(() => {
    if (unchosen && isIdle) mutate(true)
  }, [unchosen, isIdle, mutate])

  if (!s) return null
  const on = choose.isPending ? choose.variables : s.chosen ? s.on : true
  return (
    <label className="flex max-w-[640px] cursor-pointer items-start gap-2.5">
      <input
        type="checkbox"
        className="mt-[3px] h-[17px] w-[17px] shrink-0 accent-accent"
        checked={on}
        onChange={(e) => choose.mutate(e.target.checked)}
      />
      <span className="flex flex-col gap-1">
        <span className="text-[15px] text-ink">Backlog verdicts from other players</span>
        <span className="text-[13.5px] text-dim">
          What other UpNext players say about how games fit a backlog and limited time, public under CC0, on game pages and pick cards.
          A daily download from the verdicts service{s.service ? '' : ', once one is set'}; it sends nothing about you. Settings › Library data can
          turn it off.
        </span>
      </span>
    </label>
  )
}
