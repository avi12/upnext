import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { api } from '../../api'
import type { Player } from '../../types'
import { remember } from '../../lib/players'
import { ToastProvider } from '../Toast'
import { NameNudge } from './NameNudge'

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

function show(first: string, others: string[] = ['Sam']) {
  const players = [player(1, first), ...others.map((n, i) => player(i + 2, n))]
  vi.spyOn(api, 'players').mockResolvedValue({ players, current: 1, bound: false })
  remember(1)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/library']}>
          <NameNudge />
          <Routes>
            <Route path="/library" element={<p>Library</p>} />
            <Route path="/settings" element={<p>Settings page</p>} />
          </Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('the reminder about a made-up name', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    localStorage.clear()
    sessionStorage.clear()
  })

  it('points "Player 1" to Settings › Players once, in a household of two', async () => {
    const first = show('Player 1')
    expect(await screen.findByText('You’re “Player 1” here')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Change it' }))
    expect(await screen.findByText('Settings page')).toBeTruthy()
    first.unmount()

    show('Player 1')
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    await screen.findByText('Library')
    expect(screen.queryByText('You’re “Player 1” here')).toBeNull()
  })

  it('says nothing to a chosen name, or to one player, whose name shows nowhere', async () => {
    show('Alex')
    await waitFor(() => expect(api.players).toHaveBeenCalled())
    show('Player 1', [])
    await waitFor(() => expect(api.players).toHaveBeenCalledTimes(2))
    await new Promise((r) => setTimeout(r, 20))
    expect(screen.queryByText(/^You’re “/)).toBeNull()
  })
})
