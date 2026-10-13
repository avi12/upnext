import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { api } from '../../api'
import type { Pairing, PairingOutcome, SetupInfo, SetupState, Settings } from '../../types'
import { ToastProvider } from '../../components/Toast'
import { BigSetupPage } from './BigSetupPage'

const setup = {
  environment: { os: 'linux', mode: 'app', container: false, display: true, gog_window: true, client_local: true },
  stores: {
    steam: { path: '/home/you/.local/share/Steam', local_games: 120, web_api: false },
    heroic: { path: null, games: {} },
    galaxy: { exists: false },
  },
  ai: { backend: 'none', available: false, claude_token: false },
  library: { titles: 0, copies: 0, job: null },
  background: { sync_waiting: false, details_waiting: false, seed: { state: 'none' } },
  setup: { setup: 'open', step: null },
} as unknown as SetupInfo

const settings = (otherDevices: boolean) => ({ listen: { this_computer: true, other_devices: otherDevices, forced: false, error: null } }) as unknown as Settings

function Home() {
  const state = useLocation().state as { bigTour?: boolean } | null
  return <p>Curate{state?.bigTour ? ' with the tour' : ''}</p>
}

function show() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/setup']}>
          <Routes>
            <Route path="/setup" element={<BigSetupPage />} />
            <Route path="/" element={<Home />} />
          </Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('big screen’s first run', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('reads the games found, hands typing to a phone, and ends on the tour', async () => {
    vi.spyOn(api, 'setup').mockResolvedValue(setup)
    vi.spyOn(api, 'settings').mockResolvedValue(settings(true))
    vi.spyOn(api, 'steamShortcut').mockResolvedValue({ target: null, accounts: [], steam_running: false, unavailable: 'no Steam account' })
    const background = vi.spyOn(api, 'setupBackground').mockResolvedValue(setup.background)
    const pairing = vi
      .spyOn(api, 'startPairing')
      .mockResolvedValue({ code: '482913', expires_at: 4102444800, address: 'http://192.168.0.42:8787', qr_svg: '<svg/>' } as Pairing)
    const finish = vi.spyOn(api, 'updateSetup').mockResolvedValue({ needed: false } as SetupState)
    show()

    fireEvent.click(await screen.findByRole('button', { name: 'Start' }))
    expect(screen.getByText('Steam, with 120 games in its files')).toBeTruthy()
    expect(screen.getByText('Heroic: not found')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Read my games' }))
    await waitFor(() => expect(background).toHaveBeenCalled())
    expect(background.mock.calls[0][0]).toEqual({ sync: 'if_changed', details: 'after_sync' })

    fireEvent.click(await screen.findByRole('button', { name: 'Finish on your phone or computer' }))
    fireEvent.click(screen.getByRole('button', { name: 'Show the code' }))
    await waitFor(() => expect(pairing).toHaveBeenCalledWith('setup', undefined, null))
    expect(await screen.findByText('482913')).toBeTruthy()
    expect(screen.getByText('http://192.168.0.42:8787/setup')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Go on without it' }))
    fireEvent.click(screen.getByRole('button', { name: 'Show me around' }))
    expect(await screen.findByText('Curate with the tour')).toBeTruthy()
    expect(finish).toHaveBeenCalledWith({ step: 'done', setup: 'done' })
  })

  it('asks for the PIN of whoever is playing before the code, with two players', async () => {
    vi.spyOn(api, 'setup').mockResolvedValue(setup)
    vi.spyOn(api, 'settings').mockResolvedValue(settings(true))
    vi.spyOn(api, 'players').mockResolvedValue({
      players: [
        { id: 1, name: 'Alex', colour: '#4f8cff', has_pin: true, created_at: 0, default: true },
        { id: 2, name: 'Sam', colour: '#f2994a', has_pin: false, created_at: 0, default: false },
      ],
      current: 1,
      bound: false,
    })
    const pairing = vi
      .spyOn(api, 'startPairing')
      .mockResolvedValue({ code: '482913', expires_at: 4102444800, address: 'http://192.168.0.42:8787', qr_svg: '<svg/>' } as Pairing)
    show()
    fireEvent.click(await screen.findByRole('button', { name: 'Start' }))
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }))
    fireEvent.click(screen.getByRole('button', { name: 'Finish on your phone or computer' }))
    // Once the players are in, the button asks for the PIN rather than making a code.
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    await act(() => new Promise((r) => setTimeout(r, 0)))
    fireEvent.click(screen.getByRole('button', { name: 'Show the code' }))
    const pin = await screen.findByLabelText('Alex’s PIN', { selector: 'input' })
    expect(pairing).not.toHaveBeenCalled()
    fireEvent.change(pin, { target: { value: '2468' } })
    fireEvent.click(screen.getByRole('button', { name: 'Show' }))
    await waitFor(() => expect(pairing).toHaveBeenCalledWith('setup', undefined, '2468'))
    expect(await screen.findByText('482913')).toBeTruthy()
  })

  it('lets other devices reach UpNext before it makes a code', async () => {
    vi.spyOn(api, 'setup').mockResolvedValue(setup)
    vi.spyOn(api, 'settings').mockResolvedValue(settings(false))
    const save = vi.spyOn(api, 'updateSettings').mockResolvedValue(settings(true))
    show()
    fireEvent.click(await screen.findByRole('button', { name: 'Start' }))
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }))
    fireEvent.click(screen.getByRole('button', { name: 'Finish on your phone or computer' }))
    fireEvent.click(screen.getByRole('button', { name: 'Let other devices reach UpNext' }))
    await waitFor(() => expect(save).toHaveBeenCalledWith({ allow_other_devices: true }))
    expect(await screen.findByRole('button', { name: 'Show the code' })).toBeTruthy()
  })

  it('tells a code that ran out from a phone that paired', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    vi.spyOn(api, 'setup').mockResolvedValue(setup)
    vi.spyOn(api, 'settings').mockResolvedValue(settings(true))
    vi.spyOn(api, 'startPairing')
      .mockResolvedValueOnce({ code: '482913', expires_at: 4102444800, address: 'http://192.168.0.42:8787', qr_svg: '<svg/>' } as Pairing)
      .mockResolvedValueOnce({ code: '175320', expires_at: 4102444800, address: 'http://192.168.0.42:8787', qr_svg: '<svg/>' } as Pairing)
    const outcome = vi.spyOn(api, 'pairingOutcome').mockResolvedValue({ outcome: 'ended', device: null } as PairingOutcome)
    show()
    fireEvent.click(await screen.findByRole('button', { name: 'Start' }))
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }))
    fireEvent.click(screen.getByRole('button', { name: 'Finish on your phone or computer' }))
    fireEvent.click(screen.getByRole('button', { name: 'Show the code' }))
    expect(await screen.findByText('482913')).toBeTruthy()

    await act(() => vi.advanceTimersByTimeAsync(3000))
    expect(outcome).toHaveBeenCalledWith('482913')
    expect(await screen.findByText(/That code ended before a phone or computer used it/)).toBeTruthy()
    expect(screen.queryByText('482913')).toBeNull()
    expect(screen.queryByRole('heading', { name: 'Paired' })).toBeNull()

    outcome.mockResolvedValue({ outcome: 'paired', device: { id: 7, name: 'Android phone' } } as PairingOutcome)
    fireEvent.click(screen.getByRole('button', { name: 'A new code' }))
    expect(await screen.findByText('175320')).toBeTruthy()
    await act(() => vi.advanceTimersByTimeAsync(3000))
    expect(outcome).toHaveBeenLastCalledWith('175320')
    expect(await screen.findByRole('heading', { name: 'Paired' })).toBeTruthy()
  })

  it('stays on the last step when setup can’t be marked done', async () => {
    vi.spyOn(api, 'setup').mockResolvedValue(setup)
    vi.spyOn(api, 'settings').mockResolvedValue(settings(true))
    vi.spyOn(api, 'steamShortcut').mockResolvedValue({ target: null, accounts: [], steam_running: false, unavailable: 'no Steam account' })
    vi.spyOn(api, 'updateSetup').mockRejectedValue(new Error('the database is locked'))
    show()
    fireEvent.click(await screen.findByRole('button', { name: 'Start' }))
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }))
    fireEvent.click(screen.getByRole('button', { name: 'Go on without it' }))
    fireEvent.click(screen.getByRole('button', { name: 'Start picking' }))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/Setup couldn’t be finished/)).toBeTruthy()
    expect(screen.queryByText(/^Curate/)).toBeNull()
  })
})
