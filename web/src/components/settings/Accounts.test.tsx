import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { AccountsResponse, Settings, SetupInfo } from '../../types'
import { ToastProvider } from '../Toast'
import { GogRow, HeroicRow, SteamRow } from './Accounts'
import { steamIdText } from './accountText'

const ID = '76561197960287930'

function steam(change: Partial<Settings>): Settings {
  return { steam_id: ID, steam_id_source: 'app', steam_persona: null, steam_avatar: null, steam_api_key_set: false, ...change } as Settings
}

function mount(s: Settings) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <SteamRow s={s} save={vi.fn()} inWizard />
    </QueryClientProvider>,
  )
}

describe('Settings, the Steam row', () => {
  it('says how UpNext knows the SteamID', () => {
    expect(steamIdText(steam({}))).toBe(`Signed in as ${ID}`)
    expect(steamIdText(steam({ steam_persona: 'Gordon' }))).toBe('Signed in as Gordon')
    expect(steamIdText(steam({ steam_id_source: 'steam client' }))).toBe(`Found on this computer: ${ID}`)
    expect(steamIdText(steam({ steam_id_source: 'env' }))).toBe(`${ID}, from .env`)
  })

  it('tests the API key, and only once there is one', () => {
    mount(steam({}))
    expect(screen.getByText(`Signed in as ${ID}`)).toBeTruthy()
    expect(screen.getByText('No API key')).toBeTruthy()
    const test = screen.getByRole('button', { name: 'Test API key' }) as HTMLButtonElement
    expect(test.disabled).toBe(true)
    expect(screen.getByText('Paste a key below to test it.')).toBeTruthy()
  })

  // On the Windows retest the account found on this computer, beside
  // Change, ran over the name: it is a line of its own under the row, and
  // the right side says only how the key stands.
  it('says who the account is on a line of its own, not beside Change', () => {
    const persona = 'A persona name long enough to fill the row twice over'
    mount(steam({ steam_id_source: 'steam client', steam_persona: persona, steam_api_key_set: true }))
    const who = screen.getByText(`Found on this computer: ${persona}`)
    const change = screen.getByRole('button', { name: 'Done' })
    const right = change.parentElement!
    expect(right.textContent).toBe('Key savedDone')
    // Not on the line with the name, the description and Change.
    const line = right.parentElement!
    expect(line.textContent).toContain('Library, playtime and installs')
    expect(line.contains(who)).toBe(false)
  })

  it('with a key saved, the test can run', () => {
    mount(steam({ steam_api_key_set: true }))
    expect((screen.getByRole('button', { name: 'Test API key' }) as HTMLButtonElement).disabled).toBe(false)
    expect(screen.queryByText('Paste a key below to test it.')).toBeNull()
  })
})

describe('Settings, the GOG row', () => {
  afterEach(() => vi.restoreAllMocks())

  it('says a window opens, that it takes a password only, and offers pasting first', async () => {
    vi.spyOn(api, 'gog').mockResolvedValue({
      connected: false,
      user_id: null,
      username: null,
      auth_url: 'https://auth.gog.com/auth',
      login_pending: false,
      login_error: null,
    })
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <GogRow />
        </ToastProvider>
      </QueryClientProvider>,
    )
    expect(await screen.findByText(/opens GOG’s sign-in in a window of its own on this computer, which may open behind this one/)).toBeTruthy()
    expect(screen.getByText(/Google doesn’t allow signing in inside an app’s window/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'paste instead' }))
    expect(screen.getByRole('textbox', { name: 'GOG login address or code' })).toBeTruthy()
    expect(screen.queryByText(/Google doesn’t allow signing in inside an app’s window/)).toBeNull()
  })

  // As for Epic: a long GOG username, the row's status, sits in the right
  // side's own box, which goes below the name when it doesn't fit beside it.
  it('keeps a long username out of the name side', async () => {
    const long = 'Sam the very patient backlogger'
    vi.spyOn(api, 'gog').mockResolvedValue({
      connected: true,
      user_id: '1',
      username: long,
      auth_url: 'https://auth.gog.com/auth',
      login_pending: false,
      login_error: null,
    })
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ToastProvider>
          <GogRow />
        </ToastProvider>
      </QueryClientProvider>,
    )
    const right = (await screen.findByText(long)).closest('div')!
    const line = right.parentElement!
    expect(line.className).toContain('flex-wrap')
    expect(right.textContent).toBe(`${long}Disconnect`)
    expect(line.firstElementChild!.textContent).toContain('GOG')
    expect(line.firstElementChild!.contains(right)).toBe(false)
  })
})

describe('Settings and the wizard, the Heroic row', () => {
  afterEach(() => vi.restoreAllMocks())

  const SAMS = '/home/sam/.config/heroic'
  const setup = { stores: { heroic: { other_users: [SAMS] }, steam: { other_users: [] } } } as unknown as SetupInfo

  function heroic(where: { this_computer: boolean; container?: boolean }, held?: SetupInfo) {
    vi.spyOn(api, 'accounts').mockResolvedValue({ connections: [], accounts: [] } as AccountsResponse)
    const s = { heroic_dir: null, listen: { container: false, ...where } } as unknown as Settings
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    return render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <HeroicRow s={s} setup={held} />
        </ToastProvider>
      </QueryClientProvider>,
    )
  }

  it('uses the setup the wizard holds, with no request of its own', () => {
    const ask = vi.spyOn(api, 'setup')
    heroic({ this_computer: true }, setup)
    expect(screen.getByText(SAMS)).toBeTruthy()
    expect(ask).not.toHaveBeenCalled()
  })

  it('in Settings, asks once, and only at the computer UpNext runs on', async () => {
    const ask = vi.spyOn(api, 'setup').mockResolvedValue(setup)
    const { unmount } = heroic({ this_computer: true })
    expect(await screen.findByText(SAMS)).toBeTruthy()
    expect(ask).toHaveBeenCalledTimes(1)
    unmount()
    ask.mockClear()
    heroic({ this_computer: false })
    heroic({ this_computer: true, container: true })
    await waitFor(() => expect(screen.getAllByText('Not found')).toHaveLength(2))
    expect(ask).not.toHaveBeenCalled()
  })
})
