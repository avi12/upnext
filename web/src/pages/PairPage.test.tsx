import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import type { PairAsked, PairCollected } from '../types'
import { PairPage } from './PairPage'

const asked: PairAsked = { id: 'r1', secret: 's', match_code: '4821', expires_at: 0, computer: 'den', computer_id: '4f2a9c01d3e85b67' }

function show(container = false) {
  vi.spyOn(api, 'health').mockResolvedValue({ container } as Awaited<ReturnType<typeof api.health>>)
  const ask = vi.spyOn(api, 'askToPair').mockResolvedValue(asked)
  const onPaired = vi.fn()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <PairPage onPaired={onPaired} />
    </QueryClientProvider>,
  )
  return { ask, onPaired }
}

const answer = (status: PairCollected['status']) => ({ status }) as PairCollected

describe('a device asking to be allowed', () => {
  afterEach(() => vi.restoreAllMocks())

  it('asks first, shows the number the computer shows, and is in once allowed', async () => {
    vi.spyOn(api, 'pairAnswer').mockResolvedValueOnce(answer('waiting')).mockResolvedValue(answer('allowed'))
    const { ask, onPaired } = show()
    fireEvent.click(screen.getByRole('button', { name: 'Ask to be allowed' }))
    await waitFor(() => expect(ask).toHaveBeenCalled())
    expect(await screen.findByLabelText('Number 4 8 2 1')).toBeTruthy()
    expect(screen.getByText(/On den, UpNext asks whether to allow this device/)).toBeTruthy()
    await waitFor(() => expect(onPaired).toHaveBeenCalled(), { timeout: 4000 })
  })

  it('says when the computer said no, or nobody answered, and can ask again', async () => {
    vi.spyOn(api, 'pairAnswer').mockResolvedValueOnce(answer('denied')).mockRejectedValue(new ApiError(404, 'gone'))
    show()
    fireEvent.click(screen.getByRole('button', { name: 'Ask to be allowed' }))
    expect(await screen.findByText(/The computer said no/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Ask to be allowed' }))
    expect(await screen.findByText(/Nobody answered in time/)).toBeTruthy()
  })

  it('in a container, gives the command to run on its host', async () => {
    vi.spyOn(api, 'pairAnswer').mockResolvedValue(answer('waiting'))
    show(true)
    await screen.findByText(/UpNext runs in a container/)
    fireEvent.click(screen.getByRole('button', { name: 'Ask to be allowed' }))
    expect(await screen.findByText('docker exec upnext upnext pair approve 4821')).toBeTruthy()
  })

  it('still takes a code', async () => {
    show()
    fireEvent.click(screen.getByRole('button', { name: 'Type a code instead' }))
    expect(await screen.findByRole('button', { name: 'Ask the computer instead' })).toBeTruthy()
  })
})

describe('signing in with an identity provider', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    window.history.replaceState(null, '', '/')
  })

  it('offers the provider when it is set up', async () => {
    vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    show()
    const link = await screen.findByRole('link', { name: 'Sign in with Authelia' })
    expect(link.getAttribute('href')).toBe('/api/v1/oidc/login')
  })

  it('says nothing about it when it is off', async () => {
    const oidc = vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: false, name: null })
    show()
    await waitFor(() => expect(oidc).toHaveBeenCalled())
    expect(screen.queryByRole('link', { name: /Sign in with/ })).toBeNull()
  })

  it('says when the account waits to be confirmed on the computer', async () => {
    window.history.replaceState(null, '', '/#signin_waiting=this+account+waits+to+be+confirmed')
    vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    show()
    expect((await screen.findByText(/Signed in, but this account waits/)).getAttribute('role')).toBe('status')
  })

  it('shows why the last sign-in failed, once', async () => {
    window.history.replaceState(null, '', '/#signin_error=isn%27t+in+a+group+allowed')
    vi.spyOn(api, 'oidc').mockResolvedValue({ enabled: true, name: 'Authelia' })
    show()
    expect((await screen.findByRole('alert')).textContent).toBe("Signing in didn't work: isn't in a group allowed")
    expect(window.location.hash).toBe('')
  })
})
