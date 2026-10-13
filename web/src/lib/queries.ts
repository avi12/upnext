import { useQuery, type UseQueryOptions } from '@tanstack/react-query'
import { api } from '../api'
import type { Config, GogStatus, JobsResponse, Profile, Settings, SetupState } from '../types'

/** The keys of the queries many parts of the UI share, written once, so
 *  that a read and an invalidation can't name the same thing two ways. */
export const keys = {
  settings: ['settings'],
  config: ['config'],
  profile: ['profile'],
  jobs: ['jobs'],
  setupState: ['setup-state'],
  players: ['players'],
  gog: ['gog'],
} as const

/** What a caller may add to a shared query: `enabled`, a polling interval,
 *  or a fresher stale time than the usual one. */
type More<T, K extends readonly unknown[]> = Omit<UseQueryOptions<T, Error, T, K>, 'queryKey' | 'queryFn'>

/** The settings, fresh for a minute; the settings and setup pages ask for fresher. */
export function useSettings(more: More<Settings, typeof keys.settings> = {}) {
  return useQuery({ queryKey: keys.settings, queryFn: api.settings, staleTime: 60_000, ...more })
}

/** What this server has and can do, which changes rarely: fresh for five minutes. */
export function useConfig(more: More<Config, typeof keys.config> = {}) {
  return useQuery({ queryKey: keys.config, queryFn: api.config, staleTime: 300_000, ...more })
}

/** The library's figures and the taste profile, fresh for a minute. */
export function useProfile(more: More<Profile, typeof keys.profile> = {}) {
  return useQuery({ queryKey: keys.profile, queryFn: api.profile, staleTime: 60_000, ...more })
}

/** Setup and the tour, done or not, fresh for a minute. */
export function useSetupState(more: More<SetupState, typeof keys.setupState> = {}) {
  return useQuery({ queryKey: keys.setupState, queryFn: api.setupState, staleTime: 60_000, ...more })
}

/** GOG's sign-in, fresh for a minute. */
export function useGog(more: More<GogStatus, typeof keys.gog> = {}) {
  return useQuery({ queryKey: keys.gog, queryFn: api.gog, staleTime: 60_000, ...more })
}

/** The job running and the last ones. Each caller polls as often as it shows them. */
export function useJobs(more: More<JobsResponse, typeof keys.jobs> = {}) {
  return useQuery({ queryKey: keys.jobs, queryFn: () => api.jobs(), ...more })
}
