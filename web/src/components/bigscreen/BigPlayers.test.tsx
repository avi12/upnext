import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { api, ApiError } from '../../api'
import * as bigscreen from '../../lib/bigscreen'
import type { Player, PlayersResponse, SetupState } from '../../types'
import { BigPlayerButton } from './BigPlayers'

function player(id: number, name: string, has_pin = false): Player {
  return { id, name, colour: '#4f8cff', has_pin, created_at: 1790000000, default: id === 1 }
}

const state: SetupState = {
  needed: false,
  step: 'done',
  setup: 'done',
  tour: 'done',
  tour_offer: false,
  player_needed: false,
  player_step: 'done',
  player_setup: 'done',
}

function show(players: PlayersResponse, setup: Partial<SetupState> = {}) {
  vi.spyOn(api, 'players').mockResolvedValue(players)
  vi.spyOn(api, 'setupState').mockResolvedValue({ ...state, ...setup })
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route index element={<BigPlayerButton />} />
          <Route path="/setup/player" element={<p>Your own setup</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

const household = { players: [player(1, 'Alex'), player(2, 'Sam', true), player(3, 'Kit')], current: 1, bound: false }

describe('big screen’s player switcher', () => {
  afterEach(() => vi.restoreAllMocks())

  it('is not there with one player', async () => {
    show({ players: [player(1, 'Alex')], current: 1, bound: false })
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    expect(screen.queryByRole('button', { name: /^Playing as/ })).toBeNull()
  })

  it('plays as another player in one press, and asks for the PIN of one who has one', async () => {
    const choose = vi.spyOn(api, 'choosePlayer').mockImplementation(async (id) => household.players.find((p) => p.id === id)!)
    show(household)
    fireEvent.click(await screen.findByRole('button', { name: 'Playing as Alex' }))
    screen.getByRole('dialog', { name: 'Who’s playing?' })
    fireEvent.click(screen.getByRole('button', { name: 'Play as Kit' }))
    await waitFor(() => expect(choose).toHaveBeenCalledWith(3, undefined))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())

    fireEvent.click(await screen.findByRole('button', { name: /^Playing as / }))
    fireEvent.click(screen.getByRole('button', { name: /^Play as Sam/ }))
    const pin = screen.getByLabelText('Sam’s PIN')
    choose.mockRejectedValueOnce(new ApiError(403, 'wrong PIN'))
    fireEvent.change(pin, { target: { value: '1111' } })
    fireEvent.submit(pin.closest('form')!)
    expect(await screen.findByText('wrong PIN')).toBeTruthy()
    fireEvent.change(screen.getByLabelText('Sam’s PIN'), { target: { value: '2468' } })
    fireEvent.submit(screen.getByLabelText('Sam’s PIN').closest('form')!)
    await waitFor(() => expect(choose).toHaveBeenLastCalledWith(2, '2468'))
  })

  it('opens the player’s own setup outside big screen', async () => {
    const leave = vi.spyOn(bigscreen, 'setBigScreen').mockImplementation(() => {})
    const again = vi.spyOn(api, 'updateSetup').mockResolvedValue({ ...state, player_needed: true, player_step: 'you', player_setup: 'open' })
    show({ ...household, current: 3 }, { player_needed: true, player_step: 'you', player_setup: 'open' })
    fireEvent.click(await screen.findByRole('button', { name: 'Playing as Kit' }))
    fireEvent.click(await screen.findByRole('button', { name: /^Set up Kit/ }))
    await screen.findByText('Your own setup')
    expect(again).toHaveBeenCalledWith({ player_setup: 'open', player_step: 'you' })
    expect(leave).toHaveBeenCalledWith(false)
  })

  it('can’t switch on a device bound to one player', async () => {
    show({ ...household, current: 2, bound: true })
    fireEvent.click(await screen.findByRole('button', { name: 'Playing as Sam' }))
    expect(screen.getByText(/This device is Sam’s, so it can’t switch/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /^Play as/ })).toBeNull()
    expect(screen.getByRole('button', { name: /^Your own setup/ })).toBeTruthy()
  })
})
