import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { SeedStatus } from '../../types'
import { ToastProvider } from '../Toast'
import { SharedDataRow } from './SharedData'

const DEFAULT_URL = 'https://data.upnext.games/upnext-seed.json'

function status(seeded: number): SeedStatus {
  return {
    seeded,
    seed_date: seeded ? '2026-10-01' : null,
    kept: seeded ? 'upnext-seed-2026-10-01.jsonl.gz' : null,
    applies: seeded
      ? [{ applied_at: 1_790_000_000, seed_date: '2026-10-01', source: DEFAULT_URL, entries: 10000, matched: 812, written: 790, kept_own: 22 }]
      : [],
    default_url: DEFAULT_URL,
  }
}

function setup(seeded: number) {
  vi.spyOn(api, 'seed').mockResolvedValue(status(seeded))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <SharedDataRow />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return userEvent.setup()
}

describe('Settings, shared game data', () => {
  afterEach(() => vi.restoreAllMocks())

  it("says it isn't used, and applies the newest published data", async () => {
    const apply = vi.spyOn(api, 'applySeed').mockResolvedValue({ date: '2026-10-01', entries: 10000, matched: 812, written: 812, kept_own: 0, already: 0 })
    const user = setup(0)
    expect(await screen.findByText('· not used')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Use it' }))
    await user.click(screen.getByRole('button', { name: 'Apply the newest' }))
    await waitFor(() => expect(apply).toHaveBeenCalledWith(undefined))
    expect(await screen.findByText('Shared data of October 2026 applied')).toBeTruthy()
  })

  it('applies from an address, shows what is applied, and stops using it', async () => {
    const apply = vi.spyOn(api, 'applySeed').mockResolvedValue({ date: '2026-10-01', entries: 5, matched: 1, written: 1, kept_own: 0, already: 0 })
    const remove = vi.spyOn(api, 'removeSeed').mockResolvedValue({ removed: 812 })
    const user = setup(812)
    expect(await screen.findByText('· 812 games from October 2026')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Change' }))
    expect(screen.getByRole('list', { name: 'Applied' }).textContent).toContain('790 written, 812 matched, 22 of yours kept')
    await user.type(screen.getByLabelText('Or from an address'), 'https://example.com/upnext-seed.json')
    await user.click(screen.getByRole('button', { name: 'Apply' }))
    await waitFor(() => expect(apply).toHaveBeenCalledWith('https://example.com/upnext-seed.json'))
    await user.click(screen.getByRole('button', { name: 'Stop using it' }))
    await waitFor(() => expect(remove).toHaveBeenCalled())
  })
})
