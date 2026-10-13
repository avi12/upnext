import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { Player, PlayersResponse } from '../../types'
import { ToastProvider } from '../Toast'
import { PlayersSection } from './Players'

function player(id: number, name: string, has_pin = false): Player {
  return { id, name, colour: '#4f8cff', has_pin, created_at: 1790000000, default: id === 1 }
}

function show(state: PlayersResponse, thisComputer = true) {
  vi.spyOn(api, 'players').mockResolvedValue(state)
  vi.spyOn(api, 'hidden').mockResolvedValue([])
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <PlayersSection thisComputer={thisComputer} />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('Settings › Players', () => {
  afterEach(() => vi.restoreAllMocks())

  it('with one player, offers to add another', async () => {
    const add = vi.spyOn(api, 'addPlayer').mockResolvedValue(player(2, 'Sam'))
    show({ players: [player(1, 'Alex')], current: 1, bound: false })
    expect(await screen.findByText('Just you so far.')).toBeTruthy()
    fireEvent.change(screen.getByPlaceholderText('Their name'), { target: { value: 'Sam' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    await waitFor(() => expect(add).toHaveBeenCalledWith('Sam'))
    await screen.findByText('Sam added')
  })

  it('says which names UpNext made up, and that Change gives them one', async () => {
    show({ players: [player(1, 'Player 1'), player(2, 'Sam')], current: 1, bound: false })
    expect(await screen.findByText('The name UpNext gave you, which “Who’s playing?” shows. Change gives you your own.')).toBeTruthy()
    expect(screen.queryByText(/The name UpNext gave them/)).toBeNull()
  })

  it('sets your PIN, and changes it with the one you have', async () => {
    const pin = vi.spyOn(api, 'setPin').mockResolvedValue(undefined)
    show({ players: [player(1, 'Alex'), player(2, 'Sam')], current: 1, bound: false })
    fireEvent.click(await screen.findByRole('button', { name: 'Set a PIN' }))
    fireEvent.change(screen.getByLabelText('New PIN'), { target: { value: '48a21' } })
    expect((screen.getByLabelText('New PIN') as HTMLInputElement).value).toBe('4821')
    fireEvent.click(screen.getByRole('button', { name: 'Save PIN' }))
    await waitFor(() => expect(pin).toHaveBeenCalledWith(1, null, '4821'))
  })

  it('removes a player only on this computer, with their PIN', async () => {
    const remove = vi.spyOn(api, 'removePlayer').mockResolvedValue({ backup: 'upnext-manual-1.db', removed: 'Sam' })
    show({ players: [player(1, 'Alex'), player(2, 'Sam', true)], current: 1, bound: false })
    fireEvent.click(await screen.findByRole('button', { name: 'Change Sam' }))
    fireEvent.click(screen.getByRole('button', { name: 'Remove Sam…' }))
    const box = within(screen.getByRole('group', { name: 'Remove Sam' }))
    expect(box.getByText(/UpNext backs up first/)).toBeTruthy()
    const go = box.getByRole('button', { name: 'Remove Sam' }) as HTMLButtonElement
    expect(go.disabled).toBe(true)
    fireEvent.change(box.getByLabelText('Sam’s PIN'), { target: { value: '1234' } })
    fireEvent.click(go)
    await waitFor(() => expect(remove).toHaveBeenCalledWith(2, '1234'))
  })

  it("can't remove anyone from another device", async () => {
    show({ players: [player(1, 'Alex'), player(2, 'Sam')], current: 1, bound: false }, false)
    fireEvent.click(await screen.findByRole('button', { name: 'Change Sam' }))
    expect(screen.queryByRole('button', { name: /Remove Sam/ })).toBeNull()
  })

  it('on a device bound to one player, shows only their PIN', async () => {
    show({ players: [player(1, 'Alex'), player(2, 'Sam')], current: 1, bound: true })
    await screen.findByRole('button', { name: 'Set a PIN' })
    expect(screen.queryByRole('button', { name: /^Change/ })).toBeNull()
    expect(screen.queryByPlaceholderText('Their name')).toBeNull()
  })
})

describe('Settings › Players, linking a sign-in', () => {
  afterEach(() => vi.restoreAllMocks())

  it('links the next sign-in to a player, with their PIN', async () => {
    vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    const link = vi.spyOn(api, 'linkOidc').mockResolvedValue(player(2, 'Sam', true))
    show({ players: [player(1, 'Alex'), player(2, 'Sam', true)], current: 1, bound: false })
    fireEvent.click(await screen.findByRole('button', { name: 'Change Sam' }))
    const button = (await screen.findByRole('button', { name: 'Link the next sign-in' })) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Sam’s PIN'), { target: { value: '2468' } })
    fireEvent.click(button)
    await waitFor(() => expect(link).toHaveBeenCalledWith(2, '2468'))
  })

  it('confirms the account waiting, with the PIN again, or turns it away', async () => {
    vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    const confirm = vi.spyOn(api, 'confirmOidc').mockResolvedValue(player(2, 'Sam', true))
    const disarm = vi.spyOn(api, 'disarmOidc').mockResolvedValue(player(2, 'Sam', true))
    const waiting = { subject: 'u-7', name: 'Sam', username: 'sam', email: 'sam@example.org' }
    const sam = { ...player(2, 'Sam', true), oidc_link_until: Date.now() / 1000 + 300, oidc_waiting: waiting }
    show({ players: [player(1, 'Alex'), sam], current: 1, bound: false })
    fireEvent.click(await screen.findByRole('button', { name: 'Change Sam' }))
    expect(await screen.findByText('Sam, sam, sam@example.org')).toBeTruthy()
    const yes = screen.getByRole('button', { name: "It's Sam" }) as HTMLButtonElement
    expect(yes.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Sam’s PIN'), { target: { value: '2468' } })
    fireEvent.click(yes)
    await waitFor(() => expect(confirm).toHaveBeenCalledWith(2, 'u-7', '2468'))
    fireEvent.click(screen.getByRole('button', { name: 'Not them' }))
    await waitFor(() => expect(disarm).toHaveBeenCalledWith(2))
  })

  it('unlinks a linked player', async () => {
    vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    const unlink = vi.spyOn(api, 'unlinkOidc').mockResolvedValue(player(1, 'Alex'))
    show({ players: [{ ...player(1, 'Alex'), oidc: 'https://sso.den.example.org' }], current: 1, bound: false })
    fireEvent.click(await screen.findByRole('button', { name: 'Change Alex' }))
    expect(await screen.findByText('Signs in with Authelia.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Unlink' }))
    await waitFor(() => expect(unlink).toHaveBeenCalledWith(1))
  })

  it('offers nothing from another device', async () => {
    const oidc = vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    show({ players: [player(1, 'Alex')], current: 1, bound: false }, false)
    fireEvent.click(await screen.findByRole('button', { name: 'Change Alex' }))
    expect(screen.queryByRole('button', { name: 'Link the next sign-in' })).toBeNull()
    expect(oidc).not.toHaveBeenCalled()
  })
})
