import { useQuery } from '@tanstack/react-query'
import { api } from '../api'

/** A subscription's usage bars: read when something showing them opens and after an AI job ends (the job watcher refreshes them), never polled. */
export function usePlan() {
  return useQuery({ queryKey: ['aiPlan'], queryFn: api.aiPlan, staleTime: 30_000, refetchOnWindowFocus: false })
}
