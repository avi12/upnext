import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { api } from '../../api'
import { ToastProvider } from '../Toast'
import { SetupSection } from './Setup'

function show(thisComputer = true) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/settings']}>
          <Routes>
            <Route path="/settings" element={<SetupSection thisComputer={thisComputer} />} />
            <Route path="/setup" element={<p>The wizard</p>} />
          </Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  )
  fireEvent.click(screen.getByRole('button', { name: 'Restart the setup wizard' }))
}

const box = (name: RegExp) => screen.getByRole('checkbox', { name }) as HTMLInputElement

describe('Settings › Setup', () => {
  afterEach(() => vi.restoreAllMocks())

  it('keeps everything unless told otherwise', () => {
    const reset = vi.spyOn(api, 'resetSetup')
    show()
    expect((screen.getByRole('radio', { name: /Keep everything/ }) as HTMLInputElement).checked).toBe(true)
    expect(screen.queryByRole('checkbox')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Start the wizard' }))
    expect(screen.getByText('The wizard')).toBeTruthy()
    expect(reset).not.toHaveBeenCalled()
  })

  it('clears what was ticked after saying so, with the whole library taking the profile and history', async () => {
    const reset = vi.spyOn(api, 'resetSetup').mockResolvedValue({ backup: 'upnext-manual-20261009-181500.db', from_env: [] })
    show()
    fireEvent.click(screen.getByRole('radio', { name: 'Clear some things first' }))
    expect((screen.getByRole('button', { name: 'Start the wizard' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(box(/^The whole library/))
    for (const implied of [/^The taste profile/, /^Statuses and history/]) {
      expect(box(implied).checked).toBe(true)
      expect(box(implied).disabled).toBe(true)
    }
    fireEvent.click(box(/^Store sign-ins and keys/))
    expect(screen.getByText(/Only restoring that backup undoes this, and the GOG and Epic sign-ins aren't in backups/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Back up, clear and start' }))
    await screen.findByText('The wizard')
    expect(reset).toHaveBeenCalledWith({ accounts: true, taste: true, history: true, library: true })
  })

  it('shows why a reset was refused', async () => {
    const { ApiError } = await import('../../api')
    vi.spyOn(api, 'resetSetup').mockRejectedValue(new ApiError(409, 'Fetch details is running; wait for it or cancel it'))
    show()
    fireEvent.click(screen.getByRole('radio', { name: 'Clear some things first' }))
    fireEvent.click(box(/^The AI choice/))
    fireEvent.click(screen.getByRole('button', { name: 'Back up, clear and start' }))
    expect((await screen.findByRole('alert')).textContent).toContain('wait for it or cancel it')
  })

  it('only starts the wizard from another device', () => {
    show(false)
    expect(screen.queryByRole('radio')).toBeNull()
    expect(screen.getByText(/offered only on the computer UpNext runs on/)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Start the wizard' }).getAttribute('href')).toBe('/setup?again=1')
  })
})

describe('Settings › Setup with several players', () => {
  afterEach(() => vi.restoreAllMocks())

  const two = {
    players: [1, 2].map((id) => ({ id, name: id === 1 ? 'Alex' : 'Sam', colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 })),
    current: 2,
    bound: false,
  }

  function mount() {
    vi.spyOn(api, 'players').mockResolvedValue(two)
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <MemoryRouter initialEntries={['/settings']}>
            <Routes>
              <Route path="/settings" element={<SetupSection thisComputer />} />
              <Route path="/setup/player" element={<p>Your own setup</p>} />
            </Routes>
          </MemoryRouter>
        </ToastProvider>
      </QueryClientProvider>,
    )
  }

  it('runs your own setup again', async () => {
    const again = vi.spyOn(api, 'updateSetup').mockResolvedValue({
      needed: false,
      step: 'done',
      setup: 'done',
      tour: 'done',
      tour_offer: false,
      player_needed: true,
      player_step: 'you',
      player_setup: 'open',
    })
    mount()
    fireEvent.click(await screen.findByRole('button', { name: 'Run your setup again' }))
    await screen.findByText('Your own setup')
    expect(again).toHaveBeenCalledWith({ player_setup: 'open', player_step: 'you' })
  })

  it('says the sign-ins, taste profile and history it clears are only yours, and the library everyone’s', async () => {
    mount()
    await screen.findByRole('button', { name: 'Run your setup again' })
    fireEvent.click(screen.getByRole('button', { name: 'Restart the setup wizard' }))
    fireEvent.click(screen.getByRole('radio', { name: 'Clear some things first' }))
    expect(screen.getByText(/^Yours: your SteamID.*Steam’s key, IGDB and RAWG are everyone’s and stay\.$/)).toBeTruthy()
    expect(screen.getByText(/^Yours: what you told it/)).toBeTruthy()
    expect(screen.getByText(/^Yours: your statuses/)).toBeTruthy()
    expect(screen.getByText(/^Everyone’s: every game/)).toBeTruthy()
  })
})
