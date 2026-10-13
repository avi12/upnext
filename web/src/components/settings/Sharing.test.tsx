import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { AiAccount, Player, StoreAccount } from '../../types'
import { ToastProvider } from '../Toast'
import { SharingSection } from './Sharing'

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

function account(over: Partial<StoreAccount>): StoreAccount {
  return {
    id: 1,
    connection_id: 1,
    kind: 'steam',
    store: 'steam',
    name: 'Steam',
    owner_id: 1,
    owner: 'Alex',
    sharing: 'everyone',
    games: 412,
    mine: true,
    uses: true,
    plays_on: true,
    players: [],
    ...over,
  }
}

function ai(over: Partial<AiAccount>): AiAccount {
  return {
    id: 1,
    owner_id: 1,
    owner: 'Alex',
    backend: 'api-key',
    settings: {},
    keys_set: ['anthropic_api_key'],
    sharing: 'everyone',
    offered_to: [],
    mine: true,
    in_use: true,
    library: true,
    this_computer: true,
    ...over,
  }
}

function show(accounts: StoreAccount[], ais: AiAccount[]) {
  vi.spyOn(api, 'players').mockResolvedValue({ players: [player(1, 'Alex'), player(2, 'Sam'), player(3, 'Kit')], current: 1, bound: false })
  vi.spyOn(api, 'accounts').mockResolvedValue({ connections: [], accounts })
  vi.spyOn(api, 'aiAccounts').mockResolvedValue(ais)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <SharingSection />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('Settings › Sharing', () => {
  afterEach(() => vi.restoreAllMocks())

  it('lets the owner choose who an account is offered to, and who also plays on it', async () => {
    const set = vi.spyOn(api, 'setAccount').mockImplementation(async (id) => account({ id }))
    show(
      [
        account({
          sharing: 'chosen',
          players: [
            { player_id: 2, name: 'Sam', offered: true, uses: true, plays_on: false },
            { player_id: 3, name: 'Kit', offered: false, uses: true, plays_on: false },
          ],
        }),
      ],
      [ai({})],
    )
    const offered = await screen.findByRole('radiogroup', { name: 'Who Steam is offered to' })
    expect(within(offered).getByRole('radio', { name: 'Chosen players' }).getAttribute('aria-checked')).toBe('true')
    expect((screen.getByRole('checkbox', { name: 'Offer to Sam' }) as HTMLInputElement).checked).toBe(true)
    // Only who it's offered to can play on it.
    expect(screen.queryByRole('checkbox', { name: /Kit also plays on it/ })).toBeNull()

    fireEvent.click(screen.getByRole('checkbox', { name: /Sam also plays on it/ }))
    await waitFor(() => expect(set).toHaveBeenCalledWith(1, { players: [{ player_id: 2, plays_on: true }] }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Offer to Kit' }))
    await waitFor(() => expect(set).toHaveBeenCalledWith(1, { players: [{ player_id: 3, offered: true }] }))
    fireEvent.click(within(offered).getByRole('radio', { name: 'Only me' }))
    await waitFor(() => expect(set).toHaveBeenCalledWith(1, { sharing: 'owner' }))
  })

  it("turns an account offered to you on or off, and uses someone else's AI", async () => {
    const use = vi.spyOn(api, 'useAccount').mockResolvedValue(account({ id: 7, mine: false, uses: false }))
    const useAi = vi.spyOn(api, 'useAiAccount').mockResolvedValue(ai({ id: 9, mine: false, in_use: true }))
    show(
      [account({ id: 7, owner_id: 2, owner: 'Sam', mine: false, plays_on: false, games: 3 })],
      [ai({ in_use: false, library: false, this_computer: false }), ai({ id: 9, owner_id: 2, owner: 'Sam', mine: false, in_use: false, backend: 'claude-cli', this_computer: false })],
    )
    expect(await screen.findByText('Sam’s Steam')).toBeTruthy()
    expect(screen.getByText('Its games, without its hours.')).toBeTruthy()
    fireEvent.click(screen.getByRole('checkbox', { name: 'Use these games' }))
    await waitFor(() => expect(use).toHaveBeenCalledWith(7, false))
    expect(screen.getByText(/You have no AI in use/)).toBeTruthy()
    const sams = screen.getByText('Sam’s AI').closest('div.border-b') as HTMLElement
    fireEvent.click(within(sams).getByRole('button', { name: 'Use it for your picks' }))
    await waitFor(() => expect(useAi).toHaveBeenCalledWith(9))
  })

  it('offers your AI to chosen players', async () => {
    const set = vi.spyOn(api, 'setAiAccount').mockImplementation(async (id) => ai({ id }))
    show([], [ai({ sharing: 'chosen', offered_to: [2] })])
    const box = await screen.findByRole('checkbox', { name: 'Offer to Kit' })
    fireEvent.click(box)
    await waitFor(() => expect(set).toHaveBeenCalledWith(1, { offered_to: [2, 3] }))
  })
})
