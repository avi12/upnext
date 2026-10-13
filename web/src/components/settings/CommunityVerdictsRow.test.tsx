import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { CommunityStatus, Job, Settings } from '../../types'
import { ToastProvider } from '../Toast'
import { VerdictsOption } from '../setup/VerdictsOption'
import { CommunityVerdictsRow } from './CommunityVerdictsRow'

function status(over: Partial<CommunityStatus> = {}): CommunityStatus {
  return {
    on: false,
    chosen: false,
    service: true,
    licence: 'CC0-1.0',
    snapshot: { date: null, source: null, applied_at: null, checked_at: null, games: 0, verdicts: 0, matched: 0 },
    ...over,
  }
}

function mount(node: React.ReactNode, settings: Partial<Settings>, st: CommunityStatus) {
  vi.spyOn(api, 'settings').mockResolvedValue({ verdicts_receive: false, verdicts_prompt: false, ...settings } as Settings)
  vi.spyOn(api, 'communityVerdicts').mockResolvedValue(st)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>{node}</ToastProvider>
    </QueryClientProvider>,
  )
  return userEvent.setup()
}

describe("Other players' backlog verdicts in Settings and the wizard", () => {
  afterEach(() => vi.restoreAllMocks())

  it('turns them on and downloads them at once, then offers the picks prompt switch', async () => {
    const save = vi.spyOn(api, 'updateSettings').mockResolvedValue({ verdicts_receive: true, verdicts_prompt: false } as Settings)
    const run = vi.spyOn(api, 'runTask').mockResolvedValue({} as Job)
    const user = mount(<CommunityVerdictsRow />, {}, status())
    await user.click(await screen.findByRole('checkbox', { name: 'Off' }))
    await waitFor(() => expect(save).toHaveBeenCalledWith({ verdicts_receive: true }))
    await waitFor(() => expect(run).toHaveBeenCalledWith('verdicts'))
    expect(await screen.findByRole('checkbox', { name: /Let the AI read them/ })).toBeTruthy()
  })

  it('says what is kept, and that nothing downloads without a service', async () => {
    mount(
      <CommunityVerdictsRow />,
      { verdicts_receive: true },
      status({ on: true, chosen: true, service: false, snapshot: { date: '2026-10-01', source: 'the mock library', applied_at: 1, checked_at: 1, games: 28, verdicts: 300, matched: 28 } }),
    )
    expect(await screen.findByText('· 28 of your games, updated 1 October 2026')).toBeTruthy()
    expect(screen.getByText(/No verdicts service is set yet/)).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Download now' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('is on by default in the wizard, and the box turns it off', async () => {
    const save = vi.spyOn(api, 'updateSettings').mockResolvedValue({ verdicts_receive: true } as Settings)
    const user = mount(<VerdictsOption />, {}, status())
    const box = (await screen.findByRole('checkbox', { name: /Backlog verdicts from other players/ })) as HTMLInputElement
    await waitFor(() => expect(save).toHaveBeenCalledWith({ verdicts_receive: true }))
    expect(box.checked).toBe(true)
    await user.click(box)
    await waitFor(() => expect(save).toHaveBeenLastCalledWith({ verdicts_receive: false }))
  })
})
