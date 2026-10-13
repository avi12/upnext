import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../api'
import type { GameView, SecondChanceEntry, SecondChancesResponse } from '../types'
import { ToastProvider } from '../components/Toast'
import { ListsPage } from './ListsPage'

function entry(id: number, name: string): SecondChanceEntry {
  const game = { id, name, appid: null, art_square: null, header_image: null, playtime_known: false, playtime_minutes: 0 } as unknown as GameView
  return { game, stopped_at: null, reviews_since: 0, due: false }
}

function response(games: SecondChanceEntry[]): SecondChancesResponse {
  return { games, ai: { available: true, reason: null } } as unknown as SecondChancesResponse
}

/** The Second chance list with two games; the refresh after an answer waits
 *  until the test lets it through. */
function show(abandoned: GameView[] = []) {
  vi.spyOn(api, 'settings').mockResolvedValue({ wishlist: false } as Awaited<ReturnType<typeof api.settings>>)
  vi.spyOn(api, 'playLater').mockResolvedValue([])
  vi.spyOn(api, 'stats').mockResolvedValue({ by_status: {} } as Awaited<ReturnType<typeof api.stats>>)
  vi.spyOn(api, 'games').mockResolvedValue({ total: abandoned.length, items: abandoned })
  vi.spyOn(api, 'gems').mockResolvedValue({ gems: [] } as unknown as Awaited<ReturnType<typeof api.gems>>)
  vi.spyOn(api, 'stopReasons').mockResolvedValue([])
  vi.spyOn(api, 'jobs').mockResolvedValue({ current: null, recent: [] })
  const first = response([entry(1, 'Alpha Quest'), entry(2, 'Beta Saga')])
  let release = () => {}
  const refreshed = new Promise<SecondChancesResponse>((resolve) => {
    release = () => resolve(response([entry(2, 'Beta Saga')]))
  })
  vi.spyOn(api, 'secondChances').mockResolvedValueOnce(first).mockReturnValue(refreshed)
  const answer = vi.spyOn(api, 'secondChance').mockImplementation(async (id) => ({ id }) as GameView)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/lists?list=second_chance']}>
          <ListsPage />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  )
  return { answer, release }
}

const answers = (name: string) => within(screen.getByRole('group', { name: `Your answer for ${name}` }))

describe('Second chance answers', () => {
  afterEach(() => vi.restoreAllMocks())

  it('stay disabled until the refreshed list is on screen, and a double-click answers one game', async () => {
    const { answer, release } = show()
    const no = await waitFor(() => answers('Alpha Quest').getByRole('button', { name: 'Not for me' }) as HTMLButtonElement)

    fireEvent.click(no, { detail: 1 })
    await waitFor(() => expect(answer).toHaveBeenCalledWith(1, { answer: 'not_for_me' }))
    // The answer is in, but the list hasn't come back yet.
    await screen.findByText(/Alpha Quest: never recommended again/)
    expect(no.disabled).toBe(true)

    release()
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Your answer for Alpha Quest' })).toBeNull())
    // The next game moved up under the pointer; the double-click's second
    // click lands on it and does nothing.
    const next = answers('Beta Saga').getByRole('button', { name: 'Not for me' }) as HTMLButtonElement
    expect(next.disabled).toBe(false)
    fireEvent.click(next, { detail: 2 })
    expect(answer).toHaveBeenCalledTimes(1)

    // A click of its own still answers it.
    fireEvent.click(next, { detail: 1 })
    await waitFor(() => expect(answer).toHaveBeenLastCalledWith(2, { answer: 'not_for_me' }))
  })
})

describe('The Abandoned list', () => {
  afterEach(() => vi.restoreAllMocks())

  it('holds Second chance at its top, with the other abandoned games below it', async () => {
    const other = { ...entry(3, 'Gamma Road').game, override: 'abandoned', override_at: 1 } as GameView
    show([entry(1, 'Alpha Quest').game, entry(2, 'Beta Saga').game, other])
    await screen.findByRole('heading', { name: 'Second chance' })
    expect(screen.queryByRole('button', { name: /^Second chance/ })).toBeNull()
    expect(screen.getByRole('button', { name: /^Abandoned/ }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByText(/you haven’t answered for them since you last played/)).toBeTruthy()
    await screen.findByRole('heading', { name: 'The rest' })
    const rest = within(screen.getByRole('list', { name: 'Abandoned' }))
    expect(rest.getAllByText('Gamma Road').length).toBeGreaterThan(0)
    expect(rest.queryByText('Alpha Quest')).toBeNull()
  })
})
