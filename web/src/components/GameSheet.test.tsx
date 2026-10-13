import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../api'
import type { Config, GameView, Settings, VerdictConsent, VerdictForm } from '../types'
import { GameSheet } from './GameSheet'
import { ToastProvider } from './Toast'
import { MOCK_GAME as GAME } from '../test/mockGame'
import { verdictsContract } from '../test/verdictsContract'


function mount(g: GameView, mocks?: () => void) {
  // jsdom has no scrolling; the sheet scrolls to the top when it opens.
  Element.prototype.scrollTo = () => {}
  vi.spyOn(api, 'game').mockResolvedValue(g)
  vi.spyOn(api, 'config').mockResolvedValue({ linux_active: false } as Config)
  vi.spyOn(api, 'settings').mockResolvedValue({ steam_persona: null } as Settings)
  vi.spyOn(api, 'verdictForm').mockRejectedValue(new Error('not in this test'))
  vi.spyOn(api, 'seriesOf').mockResolvedValue(null as unknown as Awaited<ReturnType<typeof api.seriesOf>>)
  mocks?.()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ToastProvider>
          <GameSheet game={g} onClose={vi.fn()} />
        </ToastProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('The game sheet', () => {
  afterEach(() => vi.restoreAllMocks())

  it('names every store the game is owned on, none first', () => {
    mount(GAME)
    expect(screen.getByText(/Owned on Steam and Epic/)).toBeTruthy()
    expect(screen.queryByText(/also on/)).toBeNull()
  })

  it('has its actions as buttons of one height beside Play', () => {
    mount(GAME)
    for (const name of ['Watch trailer', 'Save for later', 'Store page']) {
      expect(screen.getByRole(name === 'Save for later' ? 'button' : 'link', { name }).className).toMatch(/btn-outline/)
    }
    expect(screen.getByRole('button', { name: /Mark as/ }).className).toMatch(/btn-outline/)
  })

  it('says how many reviews "Liked it" is from', () => {
    mount(GAME)
    expect(screen.getByText('98%')).toBeTruthy()
    expect(screen.getByText(/^600K reviews$/)).toBeTruthy()
  })

  it('offers a verdict after Beat it only once the form knows the mark', async () => {
    const g = { ...GAME, kind: 'ending' } as GameView
    const before: VerdictForm = {
      title_id: g.id,
      name: g.name,
      cant_send: null,
      answers: {},
      from: {},
      problems: [],
      mine: null,
      outdated: false,
      service: true,
      consent: { current: false } as VerdictConsent,
    }
    // The form fetched after the mark arrives only when the test says so.
    let arrive = () => {}
    const after = new Promise<VerdictForm>((resolve) => {
      arrive = () => resolve({ ...before, answers: { outcome: 'finished' }, from: { outcome: 'mark' } })
    })
    mount(g, () => {
      vi.spyOn(api, 'verdictForm').mockResolvedValueOnce(before).mockReturnValueOnce(after)
      vi.spyOn(api, 'verdictContract').mockResolvedValue(verdictsContract)
      vi.spyOn(api, 'gameVerdicts').mockRejectedValue(new Error('not in this test'))
      vi.spyOn(api, 'setOverride').mockResolvedValue({ ...g, override: 'done' } as GameView)
    })
    const user = userEvent.setup()
    await screen.findByRole('button', { name: 'Write a backlog verdict' })

    await user.click(screen.getByRole('button', { name: /Mark as/ }))
    await user.click(screen.getByRole('menuitemradio', { name: /Beat it/ }))
    await waitFor(() => expect(api.verdictForm).toHaveBeenCalledTimes(2))
    expect(screen.queryByText('Tell other backlog players how it went?')).toBeNull()

    arrive()
    await user.click(await screen.findByRole('button', { name: 'Write a verdict' }))
    expect((await screen.findByRole('button', { name: 'Finished it' })).getAttribute('aria-pressed')).toBe('true')
  })
})
