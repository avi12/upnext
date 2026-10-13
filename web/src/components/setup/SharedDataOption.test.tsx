import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { SeedProgress, SeedStatus, SetupBackground } from '../../types'
import { ToastProvider } from '../Toast'
import { SharedDataOption } from './SharedDataOption'

const DEFAULT_URL = 'https://github.com/zytocc/upnext/releases/latest/download/upnext-seed.json'
const MANIFEST = {
  upnext_seed: 1,
  date: '2026-10-01',
  count: 10000,
  licence: 'CC0-1.0',
  notice: 'Test data.',
  file: 'upnext-seed-2026-10-01.jsonl.gz',
  bytes: 1234,
  sha256: '00',
}

function status(kept: string | null = null): SeedStatus {
  return { seeded: kept ? 5 : 0, seed_date: kept ? '2026-10-01' : null, kept, applies: [], default_url: DEFAULT_URL }
}

const seed = (state: SeedProgress['state'], over: Partial<SeedProgress> = {}): SeedProgress => ({ wanted: true, state, ...over })

/** The option as the first-sync step shows it; the server applies the data,
 *  so `show` moves it along as `GET /setup` would. */
function mount({ progress = seed('waiting'), empty = true, kept = null as string | null, offer = true } = {}) {
  vi.spyOn(api, 'seed').mockResolvedValue(status(kept))
  if (offer) vi.spyOn(api, 'seedOffer').mockResolvedValue(MANIFEST)
  else
    vi.spyOn(api, 'seedOffer').mockRejectedValue(
      new ApiError(400, 'no shared game data is published at https://github.com/zytocc/upnext/releases/latest/download/upnext-seed.json yet (it answered 404)'),
    )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const tree = (p: SeedProgress) => (
    <QueryClientProvider client={client}>
      <ToastProvider>
        <SharedDataOption seed={p} empty={empty} />
      </ToastProvider>
    </QueryClientProvider>
  )
  const view = render(tree(progress))
  return { user: userEvent.setup(), show: (p: SeedProgress) => view.rerender(tree(p)), container: view.container }
}

const background = (wanted: boolean): SetupBackground => ({ sync_waiting: false, details_waiting: false, seed: seed('waiting', { wanted }) })

describe('The setup wizard, UpNext’s game data', () => {
  afterEach(() => vi.restoreAllMocks())

  it('is on by default with the newest release’s numbers, and says when the server applied it', async () => {
    const s = mount()
    const box = await screen.findByRole('checkbox', { name: /Use UpNext’s game data \(10,000 games, updated October 2026\)/ })
    expect((box as HTMLInputElement).checked).toBe(true)
    expect(screen.getByText(/Ready-made summaries and tags for popular games, free for anyone to use, so your AI only has to write the rest\./)).toBeTruthy()
    expect(screen.queryByText(/CC0|prompt/)).toBeNull()
    s.show(seed('applying'))
    expect(screen.getByText('Applying UpNext’s game data…')).toBeTruthy()
    s.show(seed('applied', { written: 790 }))
    expect(screen.getByText(/UpNext’s game data is applied: 790 of your games got summaries from it/)).toBeTruthy()
  })

  it('tells the server when it’s turned off or on again', async () => {
    const ask = vi.spyOn(api, 'setupBackground').mockImplementation(async (a) => background(a.seed ?? true))
    const s = mount()
    await s.user.click(await screen.findByRole('checkbox'))
    await waitFor(() => expect(ask).toHaveBeenCalledWith({ seed: false }))
    s.show(seed('waiting', { wanted: false }))
    expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(false)
    await s.user.click(screen.getByRole('checkbox'))
    await waitFor(() => expect(ask).toHaveBeenLastCalledWith({ seed: true }))
  })

  it('isn’t offered when no shared data is published', async () => {
    const s = mount({ offer: false })
    await waitFor(() => expect(api.seedOffer).toHaveBeenCalled())
    await new Promise((r) => setTimeout(r, 20))
    expect(s.container.textContent).toBe('')
  })

  it('says so when applying failed', async () => {
    mount({ progress: seed('failed', { error: 'reaching GitHub failed' }) })
    expect((await screen.findByRole('alert')).textContent).toMatch(
      /wasn’t applied: reaching GitHub failed\. Nothing else changed; Settings › Library data offers it again/,
    )
  })

  it('is offered before the first sync only for an empty library, and never when the data is kept or skipped', async () => {
    const idle = mount({ progress: seed('idle') })
    expect(await screen.findByRole('checkbox')).toBeTruthy()
    idle.container.remove()
    vi.restoreAllMocks()

    const synced = mount({ progress: seed('idle'), empty: false })
    await new Promise((r) => setTimeout(r, 20))
    expect(synced.container.textContent).toBe('')
    expect(api.seedOffer).not.toHaveBeenCalled()
    synced.container.remove()
    vi.restoreAllMocks()

    const kept = mount({ kept: 'upnext-seed-2026-10-01.jsonl.gz' })
    await waitFor(() => expect(api.seed).toHaveBeenCalled())
    await new Promise((r) => setTimeout(r, 20))
    expect(kept.container.textContent).toBe('')
    kept.container.remove()
    vi.restoreAllMocks()

    const skipped = mount({ progress: seed('skipped') })
    await new Promise((r) => setTimeout(r, 20))
    expect(skipped.container.textContent).toBe('')
  })
})
