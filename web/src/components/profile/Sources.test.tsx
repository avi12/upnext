import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../../api'
import type { TasteSources } from '../../types'
import { SourcesSection } from './Sources'

function sources(change: Partial<TasteSources>): TasteSources {
  return {
    library: { games: 10, played: 4, hours: 30, stores: ['steam'], last_sync: null },
    statuses: { games: 0, by: {} },
    reactions: { picks: { not_tonight: 1 }, pick_ratings: {}, stop_reasons: 0, second_chance_answers: 0 },
    answers: { favourites: 0, likes: 0, dislikes: 0, hard_nos: 0, habits: 0, routines: 0, not_really: 0 },
    diary: { on: false, rows: 0, days: 0, launches: 0, since: null, last_reading: null, error: null, paused: null },
    wishlist: { on: false, steam: 0, added: 0, suggested: 0, answered: 0, bought: 0 },
    ratings: { games: 0, by: {} },
    ...change,
  }
}

function mount(data: TasteSources, onDelete = vi.fn().mockResolvedValue(undefined)) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <SourcesSection data={data} onDelete={onDelete} onSeeAnswers={vi.fn()} onDiary={vi.fn()} />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return onDelete
}

describe('Where it comes from: ratings', () => {
  afterEach(() => vi.restoreAllMocks())

  it('gives the picks’ hit rate with your reactions', () => {
    mount(sources({ reactions: { picks: {}, pick_ratings: { good: 7, not_a_fit: 3 }, stop_reasons: 0, second_chance_answers: 0 } }))
    expect(screen.getByText(/Of the 10 picks you rated, 7 were good picks\./)).toBeTruthy()
  })

  it('lists your ratings of games, and deletes them on their own', async () => {
    vi.spyOn(api, 'tasteGameRatings').mockResolvedValue([{ title_id: 3, name: 'Celeste', stars: 5, at: 1_800_000_000 }])
    const onDelete = mount(sources({ ratings: { games: 3, by: { '5': 1, '2': 2 } } }))
    expect(screen.getByText(/3 games: 1 “Loved it” and 2 “Didn’t click”\./)).toBeTruthy()
    const row = screen.getByText('Your ratings of games').closest('li')!
    fireEvent.click(row.querySelector('button[aria-expanded]')!)
    await screen.findByText('Celeste')
    expect(screen.getByText(/5\/5, loved it/)).toBeTruthy()
    fireEvent.click(Array.from(row.querySelectorAll('button')).find((b) => b.textContent === 'Delete it')!)
    expect(screen.getByText(/clears your rating on every game you rated/)).toBeTruthy()
    fireEvent.click(Array.from(row.querySelectorAll('button')).filter((b) => b.textContent === 'Delete it').at(-1)!)
    expect(onDelete).toHaveBeenCalledWith('ratings')
  })

  it('says how to rate a game when there are none', () => {
    mount(sources({}))
    expect(screen.getByText('None yet. Rate a game you’ve played from 1 to 5 stars on its page.')).toBeTruthy()
  })
})

describe('Where it comes from: whose hours', () => {
  afterEach(() => vi.restoreAllMocks())

  it('names the accounts your hours come from when several people play here', async () => {
    const player = (id: number, name: string) => ({ id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 })
    vi.spyOn(api, 'players').mockResolvedValue({ players: [player(1, 'Alex'), player(2, 'Sam')], current: 2, bound: false })
    const account = { connection_id: 1, kind: 'steam', store: 'steam', sharing: 'everyone' as const, games: 40, players: [] }
    vi.spyOn(api, 'accounts').mockResolvedValue({
      connections: [],
      accounts: [
        { ...account, id: 1, name: 'Steam', owner_id: 2, owner: 'Sam', mine: true, uses: true, plays_on: true },
        { ...account, id: 2, name: 'Steam', owner_id: 1, owner: 'Alex', mine: false, uses: true, plays_on: true },
        { ...account, id: 3, name: 'GOG', store: 'gog', kind: 'gog', owner_id: 1, owner: 'Alex', mine: false, uses: true, plays_on: false },
      ],
    } as Awaited<ReturnType<typeof api.accounts>>)
    mount(sources({}))
    await screen.findByText(/Your hours come from your Steam and Alex’s Steam;/)
  })
})
