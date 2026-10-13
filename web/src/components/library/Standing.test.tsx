import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../../api'
import type { GameView, Settings } from '../../types'
import { ToastProvider } from '../Toast'
import { Standing } from './Standing'

function game(change: Partial<GameView>): GameView {
  return {
    id: 7,
    status: 'not_started',
    override: null,
    kind: 'ending',
    no_ending: null,
    playtime_known: true,
    playtime_minutes: 0,
    last_played: 0,
    rejections: 0,
    ...change,
  } as unknown as GameView
}

function mount(g: GameView) {
  vi.spyOn(api, 'settings').mockResolvedValue({ steam_persona: null } as Settings)
  const set = vi.spyOn(api, 'setNoEnding').mockResolvedValue(g)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ToastProvider>
          <Standing game={g} onUpdated={vi.fn()} />
        </ToastProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { set, container: view.container }
}

describe('The game sheet, whether a game has an ending', () => {
  afterEach(() => vi.restoreAllMocks())

  it('says an endless guess as a statement, with Wrong? to correct it', async () => {
    const { set } = mount(game({ kind: 'endless' }))
    expect(screen.getByText('This game can be played endlessly.')).toBeTruthy()
    expect(screen.getByText('UpNext’s guess, from its tags.')).toBeTruthy()
    expect(screen.queryByRole('switch')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Wrong?' }))
    await waitFor(() => expect(set).toHaveBeenCalledWith(7, false))
  })

  it('says nothing when UpNext takes the game to have an ending, tags or none', () => {
    const { container } = mount(game({}))
    expect(container.textContent).toBe('')
  })

  it('says what you set, with a way back to the guess', async () => {
    const { set } = mount(game({ kind: 'ending', no_ending: false }))
    expect(screen.getByText('This game has an ending.')).toBeTruthy()
    expect(screen.getByText('You said so.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Let UpNext guess' }))
    await waitFor(() => expect(set).toHaveBeenCalledWith(7, null))
  })
})

describe('Your rating on the game sheet', () => {
  afterEach(() => vi.restoreAllMocks())

  it('is offered once you have played the game, and saves 1 to 5 stars', async () => {
    const rated = vi.spyOn(api, 'rateGame').mockResolvedValue(game({ my_rating: 4 }))
    mount(game({ playtime_minutes: 90 }))
    const stars = screen.getByRole('group', { name: 'Your rating' })
    expect(stars.querySelectorAll('button')).toHaveLength(5)
    fireEvent.click(screen.getByRole('button', { name: '4 stars: Liked it' }))
    await waitFor(() => expect(rated).toHaveBeenCalledWith(7, 4))
  })

  it('is offered for a game with a status, and shows its word with a way to clear it', async () => {
    const cleared = vi.spyOn(api, 'rateGame').mockResolvedValue(game({}))
    mount(game({ override: 'done', status: 'done', my_rating: 5 }))
    expect(screen.getByRole('button', { name: '5 stars: Loved it' }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByText('Loved it')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    await waitFor(() => expect(cleared).toHaveBeenCalledWith(7, null))
  })
})
