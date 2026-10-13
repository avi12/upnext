import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../api'
import type { NetworkConnection, NetworkNearby } from '../types'
import { ConnectPage } from './ConnectPage'

const den = { id: '4f2a9c01d3e85b67', name: 'den', version: '0.1.0', address: 'http://192.168.0.42:8787' }
const sent = vi.fn()

function show(nearby: NetworkNearby, path = '/connect') {
  window.upnextApp = true
  window.ipc = { postMessage: sent }
  vi.spyOn(api, 'nearbyComputers').mockResolvedValue(nearby)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <ConnectPage />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('Use the library on another computer', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    sent.mockClear()
    delete window.upnextApp
    delete window.ipc
  })

  it('asks a computer it found, shows the number, and opens it once allowed', async () => {
    show({ this: 'kitchen', found: [den], saved: [] } as unknown as NetworkNearby)
    const asking: NetworkConnection = { status: 'waiting', match_code: '4821', computer: 'den', address: den.address, id: null } as NetworkConnection
    const connect = vi.spyOn(api, 'connectTo').mockResolvedValue(asking)
    vi.spyOn(api, 'connection')
      .mockResolvedValueOnce(asking)
      .mockResolvedValue({ ...asking, status: 'allowed', id: den.id })
    await screen.findByText('den')
    // The first is the found computer's; the last, under the address field.
    fireEvent.click(screen.getAllByRole('button', { name: 'Ask to use it' })[0])
    // With the id it was found with, so another UpNext at that address is refused.
    await waitFor(() => expect(connect).toHaveBeenCalledWith(den.address, den.id))
    expect(await screen.findByLabelText('Number 4 8 2 1')).toBeTruthy()
    await waitFor(() => expect(sent).toHaveBeenCalledWith(`connect:${den.id}`), { timeout: 4000 })
  })

  it('says when the computer said no', async () => {
    show({ this: 'kitchen', found: [den], saved: [] } as unknown as NetworkNearby)
    const asking = { status: 'waiting', match_code: '4821', computer: 'den', address: den.address, id: null } as NetworkConnection
    vi.spyOn(api, 'connectTo').mockResolvedValue(asking)
    vi.spyOn(api, 'connection').mockResolvedValue({ ...asking, status: 'denied' })
    await screen.findByText('den')
    // The first is the found computer's; the last, under the address field.
    fireEvent.click(screen.getAllByRole('button', { name: 'Ask to use it' })[0])
    expect(await screen.findByText('den said no.')).toBeTruthy()
    expect(sent).not.toHaveBeenCalled()
  })

  it('offers the saved one again, or this computer, when it did not answer', async () => {
    const saved = { id: den.id, name: 'den', address: den.address, added_at: 1 }
    show({ this: 'kitchen', found: [], saved: [saved] } as unknown as NetworkNearby, `/connect?unreachable=${den.id}`)
    expect(await screen.findByText('den didn’t answer')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(sent).toHaveBeenLastCalledWith(`connect:${den.id}`)
    fireEvent.click(screen.getAllByRole('button', { name: 'Use this computer’s library' })[0])
    expect(sent).toHaveBeenLastCalledWith('connect:here')
  })

  it('says when another UpNext answers at a saved computer’s address', async () => {
    const saved = { id: den.id, name: 'den', address: den.address, added_at: 1 }
    show({ this: 'kitchen', found: [], saved: [saved] } as unknown as NetworkNearby, `/connect?changed=${den.id}`)
    expect(await screen.findByText(`den isn’t at ${den.address} any more`)).toBeTruthy()
    expect(screen.getByText(/didn’t open it or send it den’s access/)).toBeTruthy()
  })
})
