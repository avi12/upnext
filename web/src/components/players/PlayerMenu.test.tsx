import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../../api'
import type { Player, PlayersResponse } from '../../types'
import { PlayerMenu } from './PlayerMenu'

function player(id: number, name: string, has_pin = false): Player {
  return { id, name, colour: '#4f8cff', has_pin, created_at: 1790000000, default: id === 1 }
}

function show(state: PlayersResponse) {
  vi.spyOn(api, 'players').mockResolvedValue(state)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <PlayerMenu />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

const household: PlayersResponse = { players: [player(1, 'Alex'), player(2, 'Sam'), player(3, 'Kit', true)], current: 1, bound: false }

describe('The top bar’s player menu', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    localStorage.clear()
  })

  it('switches to a player without a PIN in one click', async () => {
    const choose = vi.spyOn(api, 'choosePlayer').mockResolvedValue(player(2, 'Sam'))
    show(household)
    fireEvent.click(await screen.findByRole('button', { name: 'Playing as Alex' }))
    expect(screen.getByRole('menuitem', { name: /Alex’s profile/ })).toBeTruthy()
    fireEvent.click(screen.getByRole('menuitem', { name: /Play as Sam/ }))
    await waitFor(() => expect(choose).toHaveBeenCalledWith(2, undefined))
  })

  it('asks for the PIN of a player who has one', async () => {
    const choose = vi.spyOn(api, 'choosePlayer')
    const asked = vi.fn()
    window.addEventListener('upnext:who', asked)
    show(household)
    fireEvent.click(await screen.findByRole('button', { name: 'Playing as Alex' }))
    fireEvent.click(screen.getByRole('menuitem', { name: /Play as Kit/ }))
    expect((asked.mock.calls[0][0] as CustomEvent).detail).toBe(3)
    expect(choose).not.toHaveBeenCalled()
    window.removeEventListener('upnext:who', asked)
  })

  it('offers no switching on a device bound to one player', async () => {
    show({ ...household, bound: true })
    fireEvent.click(await screen.findByRole('button', { name: 'Playing as Alex' }))
    expect(screen.queryByRole('menuitem', { name: /Play as/ })).toBeNull()
    expect(screen.getByText(/This device is Alex’s, so it can’t switch/)).toBeTruthy()
  })
})
