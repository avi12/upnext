import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import { ToastProvider } from '../Toast'
import type { Job, SetupBackground, SetupInfo } from '../../types'
import { SyncStep } from './JobSteps'
import type { StepProps } from './parts'

const sync = { id: 3, kind: 'sync', task: 'sync', source: 'ui', status: 'running', started_at: 1_790_000_000, total: 4, done: 1, message: '', lanes: [] } as unknown as Job

function background(change: Partial<SetupBackground> = {}): SetupBackground {
  return { sync_waiting: false, details_waiting: false, seed: { wanted: true, state: 'skipped' }, ...change }
}

/** The First sync step, with `running` as the jobs list's running job. */
function mount({ running = null as Job | null, titles = 0, bg = background() } = {}) {
  vi.spyOn(api, 'jobs').mockResolvedValue({ current: running, recent: [] })
  const setup = { library: { titles, copies: titles, multi_store: 0, by_source: {}, job: running }, background: bg } as unknown as SetupInfo
  const props = { setup, next: vi.fn(), back: vi.fn(), go: vi.fn() } as unknown as StepProps
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <SyncStep {...props} />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return { props, user: userEvent.setup() }
}

describe('The setup wizard’s first sync', () => {
  afterEach(() => vi.restoreAllMocks())

  it('shows the sync running in the background, and Next doesn’t wait for it', async () => {
    const { props, user } = mount({ running: sync })
    expect(await screen.findByText(/^Syncing…/)).toBeTruthy()
    expect(screen.getByText('It carries on in the background: Next doesn’t wait for it.')).toBeTruthy()
    const next = screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement
    expect(next.disabled).toBe(false)
    await user.click(next)
    expect(props.next).toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: /Sync (now|again)/ })).toBeNull()
  })

  it('says a sync waits for the running job', async () => {
    const details = { ...sync, id: 4, kind: 'enrich', task: 'details' } as Job
    mount({ running: details, titles: 12, bg: background({ sync_waiting: true }) })
    expect(await screen.findByText('A sync starts once the running Fetch details ends.')).toBeTruthy()
  })

  it('syncs again through the background work, which queues it instead of refusing', async () => {
    const ask = vi.spyOn(api, 'setupBackground').mockResolvedValue(background({ sync_waiting: true }))
    const { user } = mount({ titles: 12 })
    await user.click(await screen.findByRole('button', { name: 'Sync again' }))
    await waitFor(() => expect(ask).toHaveBeenCalledWith({ sync: 'again' }))
  })
})
