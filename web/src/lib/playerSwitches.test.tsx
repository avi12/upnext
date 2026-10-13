import type { ReactNode } from 'react'
import { afterEach, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../api'
import type { Player } from '../types'
import { usePlayerSwitches, usePlayers } from './players'

afterEach(() => vi.restoreAllMocks())

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

// The key the shells give their page (code health B-WEB-2): the same while
// the players load, another on a switch, so the page starts afresh.
it('changes on a switch of player, not while the players load', async () => {
  let current = 1
  vi.spyOn(api, 'players').mockImplementation(async () => ({ players: [player(1, 'Alex'), player(2, 'Sam')], current, bound: false }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const { result } = renderHook(() => ({ switches: usePlayerSwitches(), playing: usePlayers().current?.id }), { wrapper })
  expect(result.current.switches).toBe(0)
  // Player 1 on the page before the switch, or the switch would be the first load.
  await waitFor(() => expect(result.current.playing).toBe(1))
  expect(result.current.switches).toBe(0)

  // Switching starts the cache over, so for a moment there is nobody.
  current = 2
  await act(async () => {
    await client.resetQueries()
  })
  await waitFor(() => expect(result.current.playing).toBe(2))
  expect(result.current.switches).toBe(1)
  // The same player loaded again is no switch.
  await act(async () => {
    await client.refetchQueries()
  })
  expect(result.current.switches).toBe(1)
})
