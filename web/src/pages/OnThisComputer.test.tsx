import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../api'
import { ToastProvider } from '../components/Toast'
import type { AccountsResponse, SetupInfo, Settings } from '../types'
import { OnThisComputer } from './PlayerSetupPage'

function setup(heroicOwner: string | null, otherUsers: string[] = []): SetupInfo {
  return {
    stores: {
      steam: {
        local_accounts: [
          { steam_id: null, persona: null, player: 'Alex' },
          { steam_id: '76561197960287931', persona: 'sam', player: null },
        ],
        other_users: [],
      },
      heroic: { path: '/home/you/.config/heroic', owner: heroicOwner, other_users: otherUsers },
    },
  } as unknown as SetupInfo
}

describe('what this computer has for a new player', () => {
  it('offers the Steam account nobody has yet, and says whose the others are', () => {
    const save = vi.fn(async () => null)
    render(<OnThisComputer setup={setup('Alex')} s={{} as Settings} save={save} />)
    expect(screen.getByText('sam').parentElement!.textContent).toBe('sam is signed in to Steam on this computer.')
    expect(screen.getByText(/A Steam account on this computer is Alex’s/)).toBeTruthy()
    expect(screen.getByText(/Heroic on this computer is Alex’s/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'That’s me' }))
    expect(save).toHaveBeenCalledWith({ steam_id: '76561197960287931' }, 'Your Steam account')
  })

  it('offers nothing once the player has their own Steam account', () => {
    render(
      <OnThisComputer setup={setup(null)} s={{ steam_id: '76561197960287931' } as Settings} save={vi.fn(async () => null)} />,
    )
    expect(screen.queryByRole('button', { name: 'That’s me' })).toBeNull()
    expect(screen.getByText(/A Steam account on this computer is Alex’s/)).toBeTruthy()
    expect(screen.queryByText(/Heroic/)).toBeNull()
  })

  it('offers another computer user’s Heroic folder to a player with none of their own', async () => {
    vi.spyOn(api, 'accounts').mockResolvedValue({ connections: [], accounts: [] } as AccountsResponse)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <OnThisComputer
            setup={setup(null, ['/home/sam/.config/heroic'])}
            s={{ steam_id: '76561197960287931' } as Settings}
            save={vi.fn(async () => null)}
          />
        </ToastProvider>
      </QueryClientProvider>,
    )
    expect(screen.getByText('/home/sam/.config/heroic')).toBeTruthy()
    expect(await screen.findByRole('button', { name: 'That’s mine' })).toBeTruthy()
  })
})
