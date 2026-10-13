import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { EpicStatus } from '../../types'
import { ToastProvider } from '../Toast'
import { EpicRow } from './Epic'

const status = (over: Partial<EpicStatus> = {}): EpicStatus => ({
  connected: false,
  display_name: null,
  auth_url: 'https://www.epicgames.com/id/login?redirectUrl=x',
  error: null,
  ...over,
})

function mount(launcherDir: string | null = null) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <EpicRow launcherDir={launcherDir} />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('Settings, the Epic sign-in', () => {
  afterEach(() => vi.restoreAllMocks())

  // A long display name is the row's status: it sits in the right side's own
  // box, which the row (flex-wrap) moves below the name when it doesn't fit,
  // rather than beside the name and over it (Windows retest, Steam's row).
  it('keeps a long display name out of the name side', async () => {
    const long = 'Sam the very patient backlogger'
    vi.spyOn(api, 'epic').mockResolvedValue(status({ connected: true, display_name: long }))
    mount()
    const right = (await screen.findByText(long)).closest('div')!
    const line = right.parentElement!
    expect(line.className).toContain('flex-wrap')
    expect(right.textContent).toBe(`${long}Disconnect`)
    expect(line.firstElementChild!.textContent).toContain('Epic Games')
    expect(line.firstElementChild!.contains(right)).toBe(false)
  })

  it('signs in with the pasted code', async () => {
    vi.spyOn(api, 'epic').mockResolvedValue(status())
    const login = vi.spyOn(api, 'epicLogin').mockResolvedValue(status({ connected: true, display_name: 'player' }))
    mount()
    expect(await screen.findByText('Not connected')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    expect(screen.getByRole('link', { name: 'sign-in page' }).getAttribute('href')).toContain('epicgames.com')
    fireEvent.change(screen.getByRole('textbox', { name: 'Epic authorization code' }), { target: { value: ' {"authorizationCode":"abc"} ' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Connect' }).at(-1)!)
    await waitFor(() => expect(login).toHaveBeenCalledWith('{"authorizationCode":"abc"}'))
    expect(await screen.findByText('player')).toBeTruthy()
  })

  it('says why a code was turned down', async () => {
    vi.spyOn(api, 'epic').mockResolvedValue(status())
    vi.spyOn(api, 'epicLogin').mockRejectedValue(new ApiError(400, 'Epic turned the code down'))
    mount()
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Epic authorization code' }), { target: { value: 'abc' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Connect' }).at(-1)!)
    expect((await screen.findByRole('alert')).textContent).toBe('Epic turned the code down')
  })

  it('shows an ended sign-in and the launcher it found', async () => {
    vi.spyOn(api, 'epic').mockResolvedValue(status({ error: 'Your Epic sign-in expired. Sign in again.' }))
    mount('C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests')
    expect(await screen.findByText('Signed out')).toBeTruthy()
    expect(screen.getByText('Your Epic sign-in expired. Sign in again.')).toBeTruthy()
    expect(screen.getByText('C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests')).toBeTruthy()
  })
})
