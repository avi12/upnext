import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { NewToken, Pairing, Player, Settings } from '../../types'
import { ToastProvider } from '../Toast'
import { DevicesSection } from './Devices'

function player(id: number, name: string, has_pin = false): Player {
  return { id, name, colour: '#4f8cff', has_pin, created_at: 1790000000, default: id === 1 }
}

const settings = {
  listen: { host: '0.0.0.0', port: 8787, this_computer: true, other_devices: true, container: false, https: false, forced: false, error: null },
  public_url: null,
  public_url_source: null,
  network_name: null,
  network_name_shown: null,
  announce: false,
  tls_cert: null,
  tls_key: null,
  oidc_issuer: null,
  oidc_client_id: null,
  oidc_client_secret_set: false,
  oidc_name: null,
  oidc_auto_provision: true,
  oidc_allowed_groups: null,
  oidc_redirect_uri: 'https://upnext.den.example.org/api/v1/oidc/callback',
  trusted_proxies: null,
  trusted_proxy_login: false,
} as unknown as Settings

function show(s: Settings = settings, current = 1, save = vi.fn()) {
  vi.spyOn(api, 'players').mockResolvedValue({ players: [player(1, 'Alex'), player(2, 'Sam', true)], current, bound: false })
  vi.spyOn(api, 'tokens').mockResolvedValue({ tokens: [], current: null })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <DevicesSection s={s} save={save} />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('Settings › Devices and API, with players', () => {
  afterEach(() => vi.restoreAllMocks())

  it('a token for a player with a PIN takes their PIN', async () => {
    const create = vi.spyOn(api, 'createToken').mockResolvedValue({ id: 3, token: 'upn_x' } as NewToken)
    show()
    fireEvent.change(screen.getByPlaceholderText('phone, Home Assistant'), { target: { value: 'tablet' } })
    fireEvent.change(await screen.findByLabelText('Acts for'), { target: { value: '2' } })
    const make = screen.getByRole('button', { name: 'Make token' }) as HTMLButtonElement
    expect(make.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Sam’s PIN'), { target: { value: '2468' } })
    expect(make.disabled).toBe(false)
    fireEvent.click(make)
    await waitFor(() => expect(create).toHaveBeenCalledWith('tablet', 'read', 2, '2468'))
  })

  it('a device for a player with a PIN takes their PIN', async () => {
    const start = vi.spyOn(api, 'startPairing').mockResolvedValue({ code: '482913', expires_at: Date.now() / 1000 + 300 } as Pairing)
    show()
    fireEvent.click(await screen.findByRole('radio', { name: 'Sam' }))
    const add = screen.getByRole('button', { name: 'Add a device' }) as HTMLButtonElement
    expect(add.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Sam’s PIN'), { target: { value: '2468' } })
    fireEvent.click(add)
    await waitFor(() => expect(start).toHaveBeenCalledWith(undefined, 2, '2468'))
  })

  it('a device for everyone needs no PIN', async () => {
    const start = vi.spyOn(api, 'startPairing').mockResolvedValue({ code: '482913', expires_at: Date.now() / 1000 + 300 } as Pairing)
    show()
    fireEvent.click(await screen.findByRole('radio', { name: 'Everyone’s' }))
    expect(screen.queryByLabelText('Sam’s PIN')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Add a device' }))
    await waitFor(() => expect(start).toHaveBeenCalledWith(undefined, 'shared', null))
  })
})

describe('Settings › Devices and API, signing in', () => {
  afterEach(() => vi.restoreAllMocks())

  it('the first player sets the provider, the secret write-only', async () => {
    const save = vi.fn().mockResolvedValue(null)
    show({ ...settings, oidc_client_secret_set: true }, 1, save)
    const issuer = await screen.findByLabelText('Issuer')
    fireEvent.change(issuer, { target: { value: 'https://sso.den.example.org' } })
    fireEvent.blur(issuer)
    await waitFor(() => expect(save).toHaveBeenCalledWith({ oidc_issuer: 'https://sso.den.example.org' }, expect.any(String), { inline: true }))
    const secret = screen.getByLabelText('Client secret') as HTMLInputElement
    expect(secret.type).toBe('password')
    expect(secret.value).toBe('')
    expect(secret.placeholder).toBe('Saved. Paste to replace')
    expect(screen.getByText('https://upnext.den.example.org/api/v1/oidc/callback')).toBeTruthy()
    // The proxy's login waits for a proxy to trust.
    expect((screen.getByLabelText(/The proxy does the login/) as HTMLInputElement).disabled).toBe(true)
  })

  it('from another device, shows them but sets nothing', async () => {
    show({ ...settings, listen: { ...settings.listen, this_computer: false }, trusted_proxies: '10.0.0.2' })
    expect(((await screen.findByLabelText('Issuer')) as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByLabelText('Their addresses') as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByLabelText(/The proxy does the login/) as HTMLInputElement).disabled).toBe(true)
  })

  it('isn’t shown to another player', async () => {
    show(settings, 2)
    await screen.findByRole('radio', { name: 'Sam' })
    expect(screen.queryByLabelText('Issuer')).toBeNull()
  })
})
