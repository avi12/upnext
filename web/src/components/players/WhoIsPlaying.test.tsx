import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Player, PlayersResponse } from '../../types'
import { memory } from '../../lib/players'
import { PlayerGate } from './WhoIsPlaying'

function player(id: number, name: string, has_pin = false): Player {
  return { id, name, colour: '#4f8cff', has_pin, created_at: 1790000000, default: id === 1 }
}

function show(state: PlayersResponse) {
  vi.spyOn(api, 'players').mockResolvedValue(state)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <PlayerGate />
    </QueryClientProvider>,
  )
}

const household: PlayersResponse = { players: [player(1, 'Alex'), player(2, 'Sam', true)], current: 1, bound: false }

describe("Who's playing?", () => {
  afterEach(() => {
    vi.restoreAllMocks()
    localStorage.clear()
    sessionStorage.clear()
  })

  it('opens with two players in a browser that has not chosen, and a card picks a player without a PIN', async () => {
    const choose = vi.spyOn(api, 'choosePlayer').mockResolvedValue(player(1, 'Alex'))
    show(household)
    await screen.findByRole('heading', { name: 'Who’s playing?' })
    fireEvent.click(screen.getByRole('button', { name: 'Alex' }))
    await waitFor(() => expect(choose).toHaveBeenCalledWith(1, undefined))
    await waitFor(() => expect(memory().last).toBe(1))
  })

  it('asks for the PIN of a player who has one, says why a wrong one failed, and takes the right one', async () => {
    const choose = vi
      .spyOn(api, 'choosePlayer')
      .mockRejectedValueOnce(new ApiError(403, 'wrong PIN'))
      .mockResolvedValueOnce(player(2, 'Sam', true))
    show(household)
    fireEvent.click(await screen.findByRole('button', { name: /^Sam/ }))
    const pin = await screen.findByLabelText('Sam’s PIN', { selector: 'input' })
    for (const d of '1111') fireEvent.click(screen.getByRole('button', { name: d }))
    fireEvent.click(screen.getByRole('button', { name: 'OK' }))
    await screen.findByText('wrong PIN')
    expect((pin as HTMLInputElement).value).toBe('')
    fireEvent.change(pin, { target: { value: '4821' } })
    fireEvent.submit(pin.closest('form')!)
    await waitFor(() => expect(choose).toHaveBeenLastCalledWith(2, '4821'))
    await waitFor(() => expect(memory().last).toBe(2))
  })

  it('adds a player and plays as them, and remembers Ask each time', async () => {
    const add = vi.spyOn(api, 'addPlayer').mockResolvedValue(player(3, 'Kit'))
    const choose = vi.spyOn(api, 'choosePlayer').mockResolvedValue(player(3, 'Kit'))
    show(household)
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Ask each time UpNext opens' }))
    expect(memory().ask).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Add a player' }))
    fireEvent.change(screen.getByLabelText('Their name'), { target: { value: ' Kit ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    await waitFor(() => expect(add).toHaveBeenCalledWith('Kit'))
    await waitFor(() => expect(choose).toHaveBeenCalledWith(3, undefined))
  })

  it('stays away with one player, once this browser has chosen, and on a bound device', async () => {
    const players = vi.spyOn(api, 'players')
    show({ players: [player(1, 'Alex')], current: 1, bound: false })
    await waitFor(() => expect(players).toHaveBeenCalled())
    expect(screen.queryByRole('heading', { name: 'Who’s playing?' })).toBeNull()
  })

  it('keeps Tab inside the picker, away from the page it covers', async () => {
    vi.spyOn(api, 'players').mockResolvedValue(household)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <button type="button">Settings</button>
        <PlayerGate />
      </QueryClientProvider>,
    )
    const dialog = await screen.findByRole('dialog', { name: 'Who’s playing?' })
    const inside = [...dialog.querySelectorAll<HTMLElement>('button, input')]
    const [first, last] = [inside[0], inside[inside.length - 1]]
    last.focus()
    fireEvent.keyDown(window, { key: 'Tab' })
    expect(document.activeElement).toBe(first)
    fireEvent.keyDown(window, { key: 'Tab', shiftKey: true })
    expect(document.activeElement).toBe(last)
    // Focus that was outside comes into the picker on the next Tab.
    screen.getByRole('button', { name: 'Settings' }).focus()
    fireEvent.keyDown(window, { key: 'Tab' })
    expect(dialog.contains(document.activeElement)).toBe(true)
  })

  it('opens when the server says nobody is chosen', async () => {
    localStorage.setItem('upnext.player', '1')
    show(household)
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    expect(screen.queryByRole('heading', { name: 'Who’s playing?' })).toBeNull()
    act(() => {
      window.dispatchEvent(new Event('upnext:choose-player'))
    })
    await screen.findByRole('heading', { name: 'Who’s playing?' })
  })

  it('goes straight to the PIN when the switcher names a player with one', async () => {
    localStorage.setItem('upnext.player', '1')
    show(household)
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    act(() => {
      window.dispatchEvent(new CustomEvent('upnext:who', { detail: 2 }))
    })
    await screen.findByLabelText('Sam’s PIN', { selector: 'input' })
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByLabelText('Sam’s PIN', { selector: 'input' })).toBeNull()
  })
})
