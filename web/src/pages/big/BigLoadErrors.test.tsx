import type { ReactNode } from 'react'
import { afterEach, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GamesResponse } from '../../types'
import { BigLibraryPage } from './BigLibraryPage'
import { BigListsPage } from './BigListsPage'

afterEach(() => vi.restoreAllMocks())

function show(page: ReactNode) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter>{page}</MemoryRouter>
    </QueryClientProvider>,
  )
}

// A request that fails isn't an empty library or list (code health B-WEB-4).
it('says the library failed to load, with Try again, never "0 games"', async () => {
  const games = vi.spyOn(api, 'games').mockRejectedValueOnce(new ApiError(0, 'Failed to fetch'))
  show(<BigLibraryPage />)
  expect((await screen.findByRole('alert')).textContent).toContain("Couldn't load your games.")
  expect(screen.getByText('Failed to fetch')).toBeTruthy()
  expect(screen.queryByText('0 games')).toBeNull()

  games.mockResolvedValue({ items: [], total: 12 } as unknown as GamesResponse)
  fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
  expect(await screen.findByText('12 games')).toBeTruthy()
  expect(screen.queryByRole('alert')).toBeNull()
})

it('says Play later and the wishlist failed to load, never that they are empty', async () => {
  vi.spyOn(api, 'playLater').mockRejectedValue(new ApiError(500, 'database is locked'))
  vi.spyOn(api, 'wishlist').mockRejectedValue(new ApiError(0, 'Failed to fetch'))
  show(<BigListsPage />)
  expect(await screen.findByText("Couldn't load Play later.")).toBeTruthy()
  expect(await screen.findByText("Couldn't load your wishlist.")).toBeTruthy()
  expect(screen.queryByText(/Nothing saved yet/)).toBeNull()
  expect(screen.queryByText('Your wishlist is empty.')).toBeNull()
  expect(screen.getAllByRole('button', { name: 'Try again' })).toHaveLength(2)
})
