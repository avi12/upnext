import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView, ReviewText } from '../../types'
import { PeopleSay, PlayerReviews } from './SheetSections'

// A summary from UpNext's shared data says so, and its reviews are
// fetched when "What players wrote" is opened.

function wrap(children: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

const REVIEW: ReviewText = { source: 'steam', positive: true, rating: null, votes: 3, hours: 20, title: null, text: 'Clever puzzles and very funny robots.', posted: null }

function game(seed: string | null, reviews: ReviewText[] = []): GameView {
  return { id: 7, digest: 'A sharp puzzle game.', review_tags: null, seed_date: seed, review_texts: reviews } as unknown as GameView
}

function open() {
  const details = screen.getByText('What players wrote').closest('details')!
  details.open = true
  fireEvent(details, new Event('toggle'))
}

describe('shared data on the game page', () => {
  afterEach(() => vi.restoreAllMocks())

  it('says where a seeded summary came from', () => {
    render(<PeopleSay game={game('2026-10-01')} />)
    expect(screen.getByText("From UpNext's shared data, October 2026")).toBeTruthy()
  })

  it("says nothing of the kind for the install's own summary", () => {
    render(<PeopleSay game={game(null)} />)
    expect(screen.queryByText(/shared data/)).toBeNull()
  })

  it('fetches the reviews of a seeded game once, when the list is opened', async () => {
    const fetch = vi.spyOn(api, 'fetchReviews').mockResolvedValue(game('2026-10-01', [REVIEW]))
    render(wrap(<PlayerReviews game={game('2026-10-01')} />))
    expect(fetch).not.toHaveBeenCalled()
    open()
    expect(await screen.findByText('Fetching what players wrote…')).toBeTruthy()
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1))
    expect(fetch).toHaveBeenCalledWith(7)
  })

  it('offers to try again when the store fails', async () => {
    vi.spyOn(api, 'fetchReviews').mockRejectedValue(new Error('Steam is down'))
    render(wrap(<PlayerReviews game={game('2026-10-01')} />))
    open()
    expect(await screen.findByText("Couldn't fetch them: Steam is down")).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Try again' })).toBeTruthy()
  })

  it("shows stored reviews as before, and nothing for a game that has none and isn't seeded", () => {
    const fetch = vi.spyOn(api, 'fetchReviews')
    const { container, rerender } = render(wrap(<PlayerReviews game={game(null, [REVIEW])} />))
    expect(screen.getByText(REVIEW.text)).toBeTruthy()
    rerender(wrap(<PlayerReviews game={game(null)} />))
    expect(container.querySelector('details')).toBeNull()
    expect(fetch).not.toHaveBeenCalled()
  })
})
