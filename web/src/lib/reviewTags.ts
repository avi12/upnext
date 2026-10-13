import { useQuery } from '@tanstack/react-query'
import { api } from '../api'
import type { ReviewTagGroup, ReviewVocabulary, Verdict } from '../types'

/** The review-tag vocabulary (GET /api/review-tags). It changes only with a
 *  new enrich, so one fetch serves the whole session. */
export function useReviewVocabulary() {
  return useQuery({ queryKey: ['review-tags'], queryFn: api.reviewTags, staleTime: 600_000 })
}

export function scaleOf(vocab: ReviewVocabulary | undefined, name: string): ReviewTagGroup | undefined {
  return vocab?.scales.find((s) => s.name === name) ?? vocab?.sets.find((s) => s.name === name)
}

/** Text marker and meaning of a verdict, so it never rests on colour alone. */
export const VERDICT: Record<Verdict, { mark: string; says: string; className: string }> = {
  praised: { mark: '+', says: 'Players praise', className: 'text-good' },
  criticized: { mark: '−', says: 'Players criticize', className: 'text-bad' },
  divisive: { mark: '±', says: 'Players disagree about', className: 'text-warn' },
}
