import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { api } from '../api'
import type { Player, SetupInfo, SetupState, Settings, StoreAccount } from '../types'
import { ToastProvider } from '../components/Toast'
import { PlayerSetupPage } from './PlayerSetupPage'

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

function state(over: Partial<SetupState> = {}): SetupState {
  return {
    needed: false,
    step: 'done',
    setup: 'done',
    tour: 'open',
    tour_offer: false,
    player_needed: true,
    player_step: null,
    player_setup: 'open',
    ...over,
  }
}

const offered: StoreAccount = {
  id: 1,
  connection_id: 1,
  kind: 'steam',
  store: 'steam',
  name: 'Steam',
  owner_id: 1,
  owner: 'Alex',
  sharing: 'everyone',
  games: 412,
  mine: false,
  uses: true,
  plays_on: false,
  players: [],
}

function show(s: SetupState) {
  vi.spyOn(api, 'setupState').mockResolvedValue(s)
  vi.spyOn(api, 'settings').mockResolvedValue({ player_keys: [] } as unknown as Settings)
  vi.spyOn(api, 'players').mockResolvedValue({ players: [player(1, 'Alex'), player(2, 'Sam')], current: 2, bound: false })
  vi.spyOn(api, 'accounts').mockResolvedValue({ accounts: [offered] } as Awaited<ReturnType<typeof api.accounts>>)
  vi.spyOn(api, 'aiAccounts').mockResolvedValue([])
  vi.spyOn(api, 'gog').mockResolvedValue({ connected: false } as Awaited<ReturnType<typeof api.gog>>)
  vi.spyOn(api, 'setup').mockResolvedValue({
    environment: { os: 'linux', mode: 'serve', container: false, display: true, gog_window: true, client_local: true },
    library: { titles: 12 },
    ai: { backend: 'openai', available: false },
  } as unknown as SetupInfo)
  const update = vi.spyOn(api, 'updateSetup').mockImplementation(async (c) => state({ ...s, player_step: c.player_step ?? s.player_step }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/setup/player']}>
          <Routes>
            <Route path="/setup/player" element={<PlayerSetupPage />} />
            <Route path="/" element={<p>Pick page</p>} />
            <Route path="/library" element={<p>Library page</p>} />
          </Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  )
  return update
}

describe("a new player's own setup", () => {
  afterEach(() => vi.restoreAllMocks())

  it('starts with their name, then lists what the others offer them, each on', async () => {
    const update = show(state())
    await screen.findByRole('heading', { name: 'Hi, Sam' })
    expect(screen.getByRole('radiogroup', { name: 'Your colour' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => expect(update).toHaveBeenCalledWith({ player_step: 'games' }))
    await screen.findByRole('heading', { name: 'Your games' })
    const row = (await screen.findByText('Alex’s Steam')).closest('div.border-b') as HTMLElement
    expect((within(row).getByRole('checkbox', { name: 'Use these games' }) as HTMLInputElement).checked).toBe(true)
  })

  it('picks up where they left off, and ends with the tour turned down', async () => {
    const update = show(state({ player_step: 'done' }))
    await screen.findByRole('heading', { name: "You're set up, Sam" })
    await screen.findByText('12 games in your library.')
    fireEvent.click(screen.getByRole('button', { name: "I'll explore" }))
    await waitFor(() => expect(update).toHaveBeenCalledWith({ player_setup: 'done', tour: 'skipped' }))
    await screen.findByText('Library page')
  })

  it('sends anyone who doesn’t need it on to the app', async () => {
    show(state({ player_needed: false, player_setup: 'done' }))
    await screen.findByText('Pick page')
  })
})
