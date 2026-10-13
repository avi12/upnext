import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GameView, HiddenTitle, Player } from '../../types'
import { ToastProvider } from '../Toast'
import { HiddenByMe } from './HiddenByMe'

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: true, created_at: 1790000000, default: id === 1 }
}

function List() {
  const hidden = useQuery({ queryKey: ['hidden'], queryFn: api.hidden, retry: false })
  // A hidden game's page, open while the list is: Lock closes it too.
  useQuery({ queryKey: ['game', 5], queryFn: () => api.game(5), retry: false })
  return <HiddenByMe hidden={hidden} onOpen={() => {}} />
}

function show() {
  vi.spyOn(api, 'players').mockResolvedValue({ players: [player(1, 'Alex'), player(2, 'Sam')], current: 1, bound: false })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <List />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

const hades: HiddenTitle = { title_id: 5, name: 'Hades', scope: 'others', at: 1790000000, own_copy: true }
const celeste: HiddenTitle = { title_id: 6, name: 'Celeste', scope: 'me', at: 1790000000, own_copy: false }

describe('Lists › Hidden by me', () => {
  afterEach(() => vi.restoreAllMocks())

  it('asks for the PIN first, then lists what you hid', async () => {
    vi.spyOn(api, 'hidden').mockRejectedValueOnce(new ApiError(403, 'enter your PIN to see the games you hid')).mockResolvedValue([hades])
    const unlock = vi.spyOn(api, 'unlockHidden').mockResolvedValue(undefined)
    show()
    const pin = await screen.findByLabelText('Your PIN', { selector: 'input' })
    fireEvent.change(pin, { target: { value: '4821' } })
    fireEvent.click(screen.getByRole('button', { name: 'Open' }))
    await waitFor(() => expect(unlock).toHaveBeenCalledWith('4821'))
    const row = within(await screen.findByRole('list', { name: 'Hidden by me' }))
    expect(row.getByText('Hades')).toBeTruthy()
    expect(row.getByText(/Hidden from everyone else/)).toBeTruthy()
  })

  it('locks again before the ten minutes are up', async () => {
    const hidden = vi.spyOn(api, 'hidden').mockResolvedValue([hades])
    const lock = vi.spyOn(api, 'lockHidden').mockResolvedValue(undefined)
    const page = vi.spyOn(api, 'game').mockResolvedValue({ id: 5, name: 'Hades' } as GameView)
    show()
    await screen.findByRole('list', { name: 'Hidden by me' })
    await waitFor(() => expect(page).toHaveBeenCalledTimes(1))
    hidden.mockRejectedValue(new ApiError(403, 'enter your PIN to see the games you hid'))
    fireEvent.click(await screen.findByRole('button', { name: 'Lock' }))
    await waitFor(() => expect(lock).toHaveBeenCalled())
    expect(await screen.findByLabelText('Your PIN', { selector: 'input' })).toBeTruthy()
    expect(screen.queryByRole('list', { name: 'Hidden by me' })).toBeNull()
    await waitFor(() => expect(page).toHaveBeenCalledTimes(2))
  })

  it('changes who a game is hidden from, and unhides it', async () => {
    vi.spyOn(api, 'hidden').mockResolvedValue([hades])
    const hide = vi.spyOn(api, 'hideGame').mockResolvedValue({ scope: 'everyone', still_seen_by: [], through_shared: [] })
    const unhide = vi.spyOn(api, 'unhideGame').mockResolvedValue(undefined)
    show()
    await screen.findByRole('list', { name: 'Hidden by me' })
    await screen.findByRole('radiogroup', { name: 'Who Hades is hidden from' })
    fireEvent.click(screen.getByRole('radio', { name: 'From everyone' }))
    await waitFor(() => expect(hide).toHaveBeenCalledWith(5, 'everyone'))
    fireEvent.click(screen.getByRole('button', { name: 'Unhide' }))
    await waitFor(() => expect(unhide).toHaveBeenCalledWith(5))
  })

  it('a game without a copy of your own can only be hidden from you', async () => {
    vi.spyOn(api, 'hidden').mockResolvedValue([hades, celeste])
    show()
    await screen.findByRole('list', { name: 'Hidden by me' })
    expect(await screen.findByRole('radiogroup', { name: 'Who Hades is hidden from' })).toBeTruthy()
    expect(screen.queryByRole('radiogroup', { name: 'Who Celeste is hidden from' })).toBeNull()
  })
})
