import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { AccountsResponse, SetupInfo, StoreConnection } from '../../types'
import { ToastProvider } from '../Toast'
import { OtherUsersFolders } from './OtherUsersFolders'

const SAMS = '/home/sam/.config/heroic'
const STEAM = '/home/sam/.local/share/Steam'

function setupWith(heroic: string[], steam: string[]): SetupInfo {
  return { stores: { heroic: { other_users: heroic }, steam: { other_users: steam } } } as unknown as SetupInfo
}

function connection(id: number, kind: string): StoreConnection {
  return { id, kind, settings: {}, signed_in: false }
}

function show(setup: SetupInfo, connections: StoreConnection[], props: { heroic?: boolean; steam?: boolean } = {}) {
  vi.spyOn(api, 'accounts').mockResolvedValue({ connections, accounts: [] } as AccountsResponse)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <OtherUsersFolders setup={setup} {...props} />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe("Other users' folders", () => {
  afterEach(() => vi.restoreAllMocks())

  it('points this player’s own Heroic connection at the folder they choose', async () => {
    const set = vi.spyOn(api, 'setConnection').mockResolvedValue(connection(4, 'heroic'))
    const add = vi.spyOn(api, 'addConnection')
    show(setupWith([SAMS], []), [connection(3, 'steam'), connection(4, 'heroic')])
    expect(screen.getByText(SAMS)).toBeTruthy()
    const mine = screen.getByRole('button', { name: 'That’s mine' })
    await waitFor(() => expect((mine as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(mine)
    await waitFor(() => expect(set).toHaveBeenCalledWith(4, { heroic_dir: SAMS }))
    expect(add).not.toHaveBeenCalled()
  })

  it('links a Heroic connection for a player who has none', async () => {
    const add = vi.spyOn(api, 'addConnection').mockResolvedValue(connection(5, 'heroic'))
    show(setupWith([SAMS], []), [])
    const mine = screen.getByRole('button', { name: 'That’s mine' })
    await waitFor(() => expect((mine as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(mine)
    await waitFor(() => expect(add).toHaveBeenCalledWith('heroic', { heroic_dir: SAMS }))
  })

  it('names another user’s Steam folder only where asked, with no button', () => {
    const { unmount } = show(setupWith([], [STEAM]), [])
    expect(screen.queryByText(STEAM)).toBeNull()
    unmount()
    show(setupWith([], [STEAM]), [], { steam: true })
    expect(screen.getByText(STEAM)).toBeTruthy()
    expect(screen.queryByRole('button')).toBeNull()
  })
})
