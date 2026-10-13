import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { Settings, SteamShortcut } from '../../types'
import { ToastProvider } from '../Toast'
import { BigScreenSection } from './BigScreen'

const shortcut = (added: boolean): SteamShortcut => ({
  target: { program: 'C:\\Users\\you\\AppData\\Local\\Programs\\UpNext\\upnext-app.exe', arguments: '--big-screen' },
  accounts: [{ account_id: '12345678', added, layout: added }],
  steam_running: false,
  unavailable: null,
})

function show(os: string, thisComputer = true) {
  const s = { os, listen: { this_computer: thisComputer } } as Settings
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <BigScreenSection s={s} />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('Settings › Big screen', () => {
  afterEach(() => vi.restoreAllMocks())

  it('adds UpNext to Steam, then says it is there', async () => {
    vi.spyOn(api, 'steamShortcut').mockResolvedValue(shortcut(false))
    const add = vi.spyOn(api, 'addToSteam').mockResolvedValue(shortcut(true))
    show('windows')
    fireEvent.click(await screen.findByRole('button', { name: 'Add to Steam' }))
    await waitFor(() => expect(add).toHaveBeenCalledTimes(1))
    expect(await screen.findByText('Added')).toBeTruthy()
  })

  it('gives the program and arguments for the other launchers, with their steps on Windows', async () => {
    vi.spyOn(api, 'steamShortcut').mockResolvedValue(shortcut(true))
    show('windows')
    expect(await screen.findByText('--big-screen')).toBeTruthy()
    expect(screen.getByText(/upnext-app\.exe$/)).toBeTruthy()
    expect(screen.getByText('The Xbox app:')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Copy: Program' })).toBeTruthy()
  })

  it('from another device, offers only big screen itself', () => {
    const look = vi.spyOn(api, 'steamShortcut')
    show('linux', false)
    expect(screen.getByRole('button', { name: 'Switch to big screen' })).toBeTruthy()
    expect(screen.queryByText('Add to Steam')).toBeNull()
    expect(look).not.toHaveBeenCalled()
  })
})
