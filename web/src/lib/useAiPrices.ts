import { useQuery } from '@tanstack/react-query'
import { api } from '../api'
import { useShowUsd } from './money'

/** The prices the AI in use is priced at, read only while dollars are shown. */
export function useAiPrices(enabled = true) {
  const show = useShowUsd()
  return useQuery({ queryKey: ['aiPrices'], queryFn: api.aiPrices, enabled: show && enabled, staleTime: 10 * 60_000 })
}
