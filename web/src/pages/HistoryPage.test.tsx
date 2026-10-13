import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../api'
import type { PickBatch } from '../types'
import { ToastProvider } from '../components/Toast'
import { HISTORY_PAGE, HistoryPage } from './HistoryPage'

function batch(n: number): PickBatch {
  return { batch_id: `b${n}`, prompt: `Request ${n}`, created_at: 1_800_000_000 - n, picks: [], spares: [], removed: [] } as unknown as PickBatch
}

function show(entry = '/history') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[entry]}>
        <ToastProvider>
          <HistoryPage />
        </ToastProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('History', () => {
  afterEach(() => vi.restoreAllMocks())

  it('pages with Show more until a short page', async () => {
    const full = Array.from({ length: HISTORY_PAGE }, (_, i) => batch(i))
    const history = vi.spyOn(api, 'history').mockResolvedValueOnce(full).mockResolvedValueOnce([batch(HISTORY_PAGE)])
    show()
    await screen.findByText('Request 0')
    expect(history).toHaveBeenLastCalledWith(HISTORY_PAGE, expect.objectContaining({ offset: 0, order: 'newest' }))
    fireEvent.click(screen.getByRole('button', { name: 'Show more' }))
    await screen.findByText(`Request ${HISTORY_PAGE}`)
    expect(history).toHaveBeenLastCalledWith(HISTORY_PAGE, expect.objectContaining({ offset: HISTORY_PAGE }))
    expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull()
  })

  it("keeps the first page's cutoff for Show more", async () => {
    const full = Array.from({ length: HISTORY_PAGE }, (_, i) => batch(i))
    const history = vi.spyOn(api, 'history').mockResolvedValueOnce(full).mockResolvedValueOnce([batch(HISTORY_PAGE)])
    show('/history?period=week')
    await screen.findByText('Request 0')
    const first = history.mock.calls[0][1]!.since
    expect(first).toBeTypeOf('number')
    // An hour later, the next page still starts its week where the first did.
    const later = Date.now() + 3_600_000
    vi.spyOn(Date, 'now').mockReturnValue(later)
    fireEvent.click(screen.getByRole('button', { name: 'Show more' }))
    await screen.findByText(`Request ${HISTORY_PAGE}`)
    expect(history).toHaveBeenLastCalledWith(HISTORY_PAGE, expect.objectContaining({ offset: HISTORY_PAGE, since: first }))
  })

  it('asks for what the sentence says, and offers every pick back when nothing matches', async () => {
    const history = vi.spyOn(api, 'history').mockResolvedValue([batch(1)])
    show('/history?period=month&order=oldest&outcome=played&q=calm')
    await screen.findByText('Request 1')
    const asked = history.mock.calls[0][1]!
    expect(asked).toMatchObject({ offset: 0, order: 'oldest', outcome: 'played', q: 'calm' })
    const month = Math.floor(Date.now() / 1000) - 30 * 86_400
    expect(Math.abs(asked.since! - month)).toBeLessThan(60)
    expect(screen.getByRole('button', { name: 'When: from the last month' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'What came of them: that you played' })).toBeTruthy()

    history.mockResolvedValue([])
    fireEvent.click(screen.getByRole('button', { name: 'What came of them: that you played' }))
    fireEvent.click(screen.getByRole('menuitemradio', { name: 'that you turned down' }))
    await screen.findByText('No picks match that.')
    expect(history).toHaveBeenLastCalledWith(HISTORY_PAGE, expect.objectContaining({ outcome: 'turned_down' }))
    history.mockResolvedValue([batch(2)])
    fireEvent.click(screen.getByRole('button', { name: 'Show all picks' }))
    await screen.findByText('Request 2')
    expect(history).toHaveBeenLastCalledWith(HISTORY_PAGE, { offset: 0, order: 'newest', q: '', outcome: undefined })
  })

  it('searches a moment after typing stops', async () => {
    const history = vi.spyOn(api, 'history').mockResolvedValue([batch(1)])
    show()
    await screen.findByText('Request 1')
    fireEvent.change(screen.getByRole('searchbox', { name: 'Find a game or a request' }), { target: { value: 'portal' } })
    await waitFor(() => expect(history).toHaveBeenLastCalledWith(HISTORY_PAGE, expect.objectContaining({ q: 'portal' })))
  })

  it('rates a pick in place, and counts the lineup’s ratings', async () => {
    const pick = {
      id: 5,
      name: 'Hades',
      reason: 'fits',
      owned: true,
      game: { id: 5, name: 'Hades', owned_on: ['steam'], primary_source: 'steam', copies: [] },
      rating: null,
    }
    const before = { ...batch(1), picks: [pick] } as unknown as PickBatch
    const after = { ...before, picks: [{ ...pick, rating: { verdict: 'not_a_fit', reason: null, at: 1 } }] } as unknown as PickBatch
    const history = vi.spyOn(api, 'history').mockResolvedValue([before])
    const rate = vi.spyOn(api, 'ratePick').mockResolvedValue(after)
    show()
    await screen.findByText('Request 1')
    fireEvent.click(screen.getByRole('button', { name: 'Not a fit' }))
    await waitFor(() => expect(rate).toHaveBeenCalledWith('b1', { title_id: 5, verdict: 'not_a_fit', reason: undefined }))
    await screen.findByRole('group', { name: 'Why it wasn\'t a fit' })
    expect(screen.getByRole('button', { name: 'Not a fit' }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByText(/· 1 not a fit/)).toBeTruthy()
    expect(history).toHaveBeenCalledTimes(1)
  })

  it('says how to get picks when there are none at all', async () => {
    vi.spyOn(api, 'history').mockResolvedValue([])
    show()
    await screen.findByText(/No picks yet/)
    expect(screen.queryByRole('searchbox')).toBeNull()
  })
})
