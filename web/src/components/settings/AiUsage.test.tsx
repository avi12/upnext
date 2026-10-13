import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { AccountUsage, AiTotals, AiUsageSummary, Player } from '../../types'
import { AiUsageSection } from './AiUsage'

function totals(calls: number): AiTotals {
  return {
    calls,
    failed: 0,
    input_tokens: calls * 1000,
    output_tokens: calls * 100,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cache_write_1h_tokens: 0,
    cost_usd: null,
    api_usd: null,
    would_cost_usd: null,
    items: 0,
    duration_ms: calls * 1000,
  }
}

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

function show(players: Player[], byAccount: AccountUsage[]) {
  const calls = byAccount.reduce((n, a) => n + a.calls, 0)
  const summary: AiUsageSummary = {
    days: 30,
    backend: 'api-key',
    priced: true,
    today: totals(calls),
    last_7_days: totals(calls),
    total: totals(calls),
    by_day: [{ day: '2026-10-10', ...totals(calls), by_kind: [{ kind: 'pick', ...totals(calls) }] }],
    by_kind: [{ kind: 'pick', ...totals(calls) }],
    by_model: [{ backend: 'api-key', model: 'claude-sonnet-5-5', ...totals(calls) }],
    by_player: [],
    by_account: byAccount,
  }
  vi.spyOn(api, 'aiUsage').mockResolvedValue(summary)
  vi.spyOn(api, 'players').mockResolvedValue({ players, current: 2, bound: false })
  vi.spyOn(api, 'aiPlan').mockResolvedValue({ backend: 'api-key', subscription: false, plan: null, reading: null, error: null, measured: null })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <AiUsageSection />
    </QueryClientProvider>,
  )
}

// The chart measures its width; jsdom has no ResizeObserver.
class NoResize {
  observe() {}
  disconnect() {}
}

describe('Settings › AI usage', () => {
  beforeEach(() => vi.stubGlobal('ResizeObserver', NoResize))
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('breaks the calls out by AI account with several players, saying whose each one is', async () => {
    show(
      [player(1, 'Alex'), player(2, 'Sam')],
      [
        { ai_account_id: 4, owner: 'Sam', mine: true, backend: 'api-key', this_computer: false, ...totals(2) },
        { ai_account_id: 1, owner: 'Alex', mine: false, backend: 'claude-cli', this_computer: true, ...totals(1) },
        { ai_account_id: 3, owner: null, mine: false, backend: null, this_computer: false, ...totals(1) },
      ],
    )
    const table = (await screen.findByText(/^By AI account/)).closest('table')!
    const rows = within(table).getAllByRole('row').slice(1)
    expect(rows).toHaveLength(3)
    const head = (i: number) => within(rows[i]).getByRole('rowheader').textContent
    expect(head(0)).toMatch(/Yours$/)
    expect(head(1)).toMatch(/on this computer.*Alex’s, shared with you/)
    expect(head(2)).toMatch(/^A removed AI account/)
    expect(within(rows[0]).getAllByRole('cell')[0].textContent).toBe('2 calls')
  })

  it('has no table by account with one player and one account', async () => {
    show([player(2, 'Sam')], [{ ai_account_id: 1, owner: 'Sam', mine: true, backend: 'claude-cli', this_computer: true, ...totals(3) }])
    await screen.findByText('Last 30 days')
    expect(screen.queryByText(/^By AI account/)).toBeNull()
  })
})
