import { useQuery } from '@tanstack/react-query'
import { api } from '../api'
import type { TaskId } from '../types'

/** The tasks' names, as the server gives them in the Jobs table; these until it has. */
const FALLBACK: Record<TaskId, string> = {
  sync: 'Sync libraries',
  details: 'Fetch details',
  reviews: 'Refresh player reviews',
  digests: 'People say, Vibes and tags',
  series_guides: 'Series guides',
  second_chances: 'Second chances',
  hidden_gems: 'Find hidden gems',
  wishlist: 'Update the wishlist',
  taste: 'Update your profile',
  hltb: 'HowLongToBeat',
  backup: 'Back up the database',
  verdicts: 'Backlog verdicts',
}

/** Each task's name, for naming jobs outside the Jobs table (Activity, toasts). */
export function useTaskLabels(): Record<TaskId, string> {
  const scheduled = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled, staleTime: 60_000 })
  const rows = scheduled.data?.tasks ?? []
  return { ...FALLBACK, ...Object.fromEntries(rows.map((r) => [r.task, r.label])) }
}
