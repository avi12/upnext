import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useEffect } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation } from 'react-router'
import { api } from '../api'
import type { Config, GamesQuery, ReviewVocabulary, Settings, Stats } from '../types'
import { LibraryPage } from './LibraryPage'

const STATS = {
  total: 40,
  staples: 2,
  rated: 6,
  installed: 5,
  multi_store: 3,
  by_status: { untouched: 10, sampled: 4, shelved: 6, done: 8 },
  by_source: { steam: 30, gog: 10, epic: 0, amazon: 0, origin: 4 },
  genres: [{ name: 'RPG', count: 12 }],
  tags: [{ name: 'Roguelike', count: 7 }],
} as unknown as Stats

const VOCAB: ReviewVocabulary = {
  tagged: 20,
  scales: [
    {
      name: 'difficulty',
      known: 15,
      values: [
        { value: 'hard', label: 'Hard', count: 6 },
        { value: 'easy', label: 'Easy', count: 0 },
      ],
    },
  ],
  sets: [],
  aspects: [{ name: 'story', praised: 9, criticized: 2, divisive: 1 }],
} as unknown as ReviewVocabulary

const seen = { search: '' }
function Where() {
  const { search } = useLocation()
  useEffect(() => {
    seen.search = search
  }, [search])
  return null
}

function show(url: string, linux = false) {
  vi.spyOn(api, 'config').mockResolvedValue({ linux_active: linux } as Config)
  vi.spyOn(api, 'stats').mockResolvedValue(STATS)
  vi.spyOn(api, 'reviewTags').mockResolvedValue(VOCAB)
  vi.spyOn(api, 'profile').mockResolvedValue({ game_count: 40 } as Awaited<ReturnType<typeof api.profile>>)
  const games = vi.spyOn(api, 'games').mockResolvedValue({ total: 0, items: [] })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <LibraryPage />
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  const lastQuery = () => games.mock.lastCall?.[0] as GamesQuery
  return { games, lastQuery, user: userEvent.setup() }
}

describe('Library filters', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => vi.restoreAllMocks())

  it('starts on unplayed games, best reviewed first, in covers', async () => {
    const { lastQuery } = show('/library')
    await waitFor(() => expect(lastQuery()).toBeDefined())
    expect(lastQuery()).toMatchObject({ status: ['untouched', 'sampled'], sort: 'review', order: 'desc', limit: 96, offset: 0 })
    expect(lastQuery().source).toBeUndefined()
    expect(screen.getByRole('button', { name: /Covers/ }).getAttribute('aria-pressed')).toBe('true')
    expect(await screen.findByRole('option', { name: 'Unplayed (14)' })).toBeTruthy()
  })

  it('reads the filters from the address, and the table has its own defaults', async () => {
    const { lastQuery } = show('/library?view=table&store=gog&sort=hltb&genre=RPG&installed=1&difficulty=hard&praised=story&q=dark')
    await waitFor(() => expect(lastQuery()).toBeDefined())
    expect(lastQuery()).toMatchObject({
      source: ['gog'],
      sort: 'hltb',
      order: 'asc',
      genre: 'RPG',
      installed: true,
      facet: ['difficulty:hard'],
      praised: ['story'],
    })
    // The table shows everything; the search waits for its debounce.
    expect(lastQuery().status).toBeUndefined()
    await waitFor(() => expect(lastQuery().q).toBe('dark'))
    expect(screen.getByText('“dark”')).toBeTruthy()
    expect(localStorage.getItem('upnext.library.view')).toBe('table')
  })

  it('offers the stores Galaxy brought in, by name', async () => {
    const { lastQuery, user } = show('/library')
    await waitFor(() => expect(lastQuery()).toBeDefined())
    const stores = screen.getByRole('combobox', { name: 'Store' })
    await screen.findByRole('option', { name: 'EA app (4)' })
    expect([...stores.querySelectorAll('option')].map((o) => o.textContent)).toEqual([
      'any library',
      'Steam (30)',
      'GOG (10)',
      'Epic (0)',
      'Amazon (0)',
      'EA app (4)',
    ])
    await user.selectOptions(stores, 'origin')
    await waitFor(() => expect(lastQuery().source).toEqual(['origin']))
    expect(new URLSearchParams(seen.search).get('store')).toBe('origin')
  })

  it('maps the store sort to the API name, staples to their flag, and drops a Linux filter with Linux off', async () => {
    const { lastQuery } = show('/library?status=staples&sort=store&order=desc&linux=native')
    await waitFor(() => expect(lastQuery()).toBeDefined())
    expect(lastQuery()).toMatchObject({ staple: true, sort: 'source', order: 'desc' })
    expect(lastQuery().status).toBeUndefined()
    expect(lastQuery().linux).toBeUndefined()
    expect(screen.queryByRole('combobox', { name: 'Linux support' })).toBeNull()
  })

  it('keeps the Linux filter and sort when Linux is on', async () => {
    const { lastQuery } = show('/library?linux=native&sort=linux', true)
    await waitFor(() => expect(lastQuery()).toBeDefined())
    expect(lastQuery()).toMatchObject({ linux: 'native', sort: 'linux', order: 'asc' })
    expect(screen.getByRole('combobox', { name: 'Linux support' })).toBeTruthy()
  })

  it('filters and sorts by your rating, and shows it on covers and in the table', async () => {
    const { lastQuery, games, user } = show('/library?view=table&rating=high&sort=rating')
    games.mockResolvedValue({
      total: 1,
      items: [{ id: 3, name: 'Celeste', status: 'done', owned_on: ['steam'], my_rating: 5, playtime_known: true, playtime_minutes: 600 }],
    } as unknown as Awaited<ReturnType<typeof api.games>>)
    await waitFor(() => expect(lastQuery()).toMatchObject({ rating: 'high', sort: 'rating', order: 'desc' }))
    expect(screen.getByRole('combobox', { name: 'Your rating' })).toBeTruthy()
    expect(await screen.findByRole('option', { name: 'rated by you (6)' })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Yours/ }).closest('th')?.getAttribute('aria-sort')).toBe('descending')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Your rating' }), 'unrated')
    await waitFor(() => expect(lastQuery().rating).toBe('unrated'))
    expect(new URLSearchParams(seen.search).get('rating')).toBe('unrated')
    expect(await screen.findByText('your rating: 5 of 5')).toBeTruthy()
  })

  it("filters by what other backlog players said, and the table shows their headline", async () => {
    vi.spyOn(api, 'settings').mockResolvedValue({ verdicts_receive: true } as Settings)
    const { lastQuery, games, user } = show('/library?view=table&backlog=respects_time')
    games.mockResolvedValue({
      total: 1,
      items: [
        {
          id: 4,
          name: 'Hades',
          status: 'untouched',
          owned_on: ['steam'],
          playtime_known: true,
          playtime_minutes: 0,
          backlog: { n: 23, headline: 'Worth it on a backlog, say 18 of 23 players', short: 'Worth it, 18 of 23', lines: [], facets: [] },
        },
      ],
    } as unknown as Awaited<ReturnType<typeof api.games>>)
    await waitFor(() => expect(lastQuery()).toMatchObject({ backlog: 'respects_time' }))
    const filter = await screen.findByRole('combobox', { name: 'Backlog players say' })
    expect(await screen.findByText('Worth it, 18 of 23')).toBeTruthy()
    expect(screen.getByRole('columnheader', { name: 'Backlog players' })).toBeTruthy()
    await user.selectOptions(filter, 'clicks_fast')
    await waitFor(() => expect(lastQuery().backlog).toBe('clicks_fast'))
    expect(new URLSearchParams(seen.search).get('backlog')).toBe('clicks_fast')
  })

    it('writes choices to the address and resets them', async () => {
    const { lastQuery, user } = show('/library')
    await waitFor(() => expect(lastQuery()).toBeDefined())
    await user.selectOptions(screen.getByRole('combobox', { name: 'Which games' }), 'shelved')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Sort by' }), 'name:asc')
    await waitFor(() => expect(lastQuery()).toMatchObject({ status: ['shelved'], sort: 'name', order: 'asc' }))
    expect(new URLSearchParams(seen.search).get('status')).toBe('shelved')

    await user.click(screen.getByRole('button', { name: 'More filters' }))
    await user.selectOptions(screen.getByRole('combobox', { name: 'Difficulty' }), 'hard')
    await waitFor(() => expect(lastQuery().facet).toEqual(['difficulty:hard']))
    // An unused value isn't offered.
    expect(screen.queryByRole('option', { name: /easy/ })).toBeNull()
    expect(screen.getByText(/Reviews speak to difficulty in 15 of your games/)).toBeTruthy()

    await user.click(screen.getByRole('button', { name: 'Reset filters' }))
    await waitFor(() => expect(lastQuery()).toMatchObject({ status: ['untouched', 'sampled'], facet: [] }))
    expect(new URLSearchParams(seen.search).get('status')).toBeNull()
    expect(new URLSearchParams(seen.search).get('sort')).toBe('name')
  })

  it('has its own search box below the top bar\'s, which finds games by name and answers /', async () => {
    // jsdom lays nothing out, so the box counts as on screen only when told.
    vi.spyOn(Element.prototype, 'getClientRects').mockReturnValue([{}] as unknown as DOMRectList)
    const { lastQuery, user } = show('/library')
    const box = screen.getByRole('searchbox', { name: 'Search your library' })
    expect(await screen.findByPlaceholderText('Find a game in 40…')).toBe(box)

    await user.keyboard('/')
    expect(document.activeElement).toBe(box)
    await user.keyboard('dark')
    await waitFor(() => expect(lastQuery().q).toBe('dark'))
    expect(new URLSearchParams(seen.search).get('q')).toBe('dark')

    await user.keyboard('{Escape}')
    await waitFor(() => expect(lastQuery().q).toBeUndefined())
    expect(new URLSearchParams(seen.search).get('q')).toBeNull()
  })
})
