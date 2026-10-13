import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Player, SetupInfo } from '../../types'
import { memory } from '../../lib/players'
import type { StepProps } from './parts'
import { PlayersStep } from './PlayersStep'

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

function mount(you: SetupInfo['you'] = { id: 1, name: 'Alex', placeholder: false, suggested: null }, bound = false) {
  vi.spyOn(api, 'players').mockResolvedValue({ players: [player(1, you.name)], current: 1, bound })
  const next = vi.fn()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const setup = { you } as SetupInfo
  render(
    <QueryClientProvider client={client}>
      <PlayersStep {...({ setup, next, back: vi.fn() } as unknown as StepProps)} />
    </QueryClientProvider>,
  )
  return next
}

describe('Who plays here?', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    localStorage.clear()
    sessionStorage.clear()
  })

  it('goes on without adding or renaming anyone when no name is typed', async () => {
    const add = vi.spyOn(api, 'addPlayer')
    const change = vi.spyOn(api, 'changePlayer')
    const next = mount()
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    expect((screen.getByLabelText(/^Your name/) as HTMLInputElement).value).toBe('Alex')
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }))
    expect(next).toHaveBeenCalled()
    expect(add).not.toHaveBeenCalled()
    expect(change).not.toHaveBeenCalled()
  })

  it('offers a name in place of the made-up "Player 1", and saves it', async () => {
    const change = vi.spyOn(api, 'changePlayer').mockResolvedValue(player(1, 'Sam'))
    const next = mount({ id: 1, name: 'Player 1', placeholder: true, suggested: 'Sam' })
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    const field = screen.getByLabelText(/^Your name/) as HTMLInputElement
    expect(field.value).toBe('Sam')
    expect(screen.getByText(/Left empty, you stay Player 1\./)).toBeTruthy()
    fireEvent.change(field, { target: { value: ' Kit ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => expect(next).toHaveBeenCalled())
    expect(change).toHaveBeenCalledWith(1, { name: 'Kit' })
  })

  it('keeps "Player 1" when the name is left empty', async () => {
    const change = vi.spyOn(api, 'changePlayer')
    const next = mount({ id: 1, name: 'Player 1', placeholder: true, suggested: null })
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    const field = screen.getByLabelText(/^Your name/) as HTMLInputElement
    expect([field.value, field.placeholder]).toEqual(['', 'Player 1'])
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    expect(next).toHaveBeenCalled()
    expect(change).not.toHaveBeenCalled()
  })

  it('asks no name on a device bound to one player, which cannot rename anyone', async () => {
    const change = vi.spyOn(api, 'changePlayer')
    const next = mount({ id: 1, name: 'Player 1', placeholder: true, suggested: 'Sam' }, true)
    await waitFor(() => expect(screen.queryByLabelText(/^Your name/)).toBeNull())
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    expect(next).toHaveBeenCalled()
    expect(change).not.toHaveBeenCalled()
  })

  it('stays on the step when the name is taken', async () => {
    vi.spyOn(api, 'changePlayer').mockRejectedValue(new ApiError(400, 'there is already a player called Sam'))
    const next = mount({ id: 1, name: 'Player 1', placeholder: true, suggested: 'Sam' })
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await screen.findByText('there is already a player called Sam')
    expect(next).not.toHaveBeenCalled()
  })

  it('adds each name, and whoever runs setup stays the one playing here', async () => {
    const add = vi.spyOn(api, 'addPlayer').mockImplementation(async (name) => player(name === 'Sam' ? 2 : 3, name))
    const next = mount()
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    fireEvent.change(screen.getByLabelText('Their name'), { target: { value: ' Sam ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Someone else' }))
    fireEvent.change(screen.getByLabelText('And'), { target: { value: 'Kit' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add 2 players' }))
    await waitFor(() => expect(next).toHaveBeenCalled())
    expect(add.mock.calls.map((c) => c[0])).toEqual(['Sam', 'Kit'])
    expect(memory()).toMatchObject({ last: 1, chosenHere: true })
  })

  it('stops at a name that is taken, keeping it to fix, with the ones before it added', async () => {
    const add = vi
      .spyOn(api, 'addPlayer')
      .mockResolvedValueOnce(player(2, 'Sam'))
      .mockRejectedValueOnce(new ApiError(400, 'there is already a player called Alex'))
    const next = mount()
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    fireEvent.change(screen.getByLabelText('Their name'), { target: { value: 'Sam' } })
    fireEvent.click(screen.getByRole('button', { name: 'Someone else' }))
    fireEvent.change(screen.getByLabelText('And'), { target: { value: 'Alex' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add 2 players' }))
    await screen.findByText('there is already a player called Alex')
    expect(add).toHaveBeenCalledTimes(2)
    expect(next).not.toHaveBeenCalled()
    expect((screen.getByLabelText('Their name') as HTMLInputElement).value).toBe('Alex')
  })
})
