import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../api'
import type { GameView } from '../types'
import { StatusMenu } from './StatusMenu'

const REASONS = [
  { code: 'busy', label: 'Got busy', group: 'life' },
  { code: 'grind', label: 'Too grindy', group: 'game' },
]

function wrap(children: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

function setup(game: Pick<GameView, 'override' | 'override_at' | 'kind'>) {
  vi.spyOn(api, 'settings').mockResolvedValue({ abandon_after_days: 365, burnout_days: 90 } as Awaited<ReturnType<typeof api.settings>>)
  vi.spyOn(api, 'stopReasons').mockResolvedValue(REASONS as Awaited<ReturnType<typeof api.stopReasons>>)
  const onChange = vi.fn()
  render(wrap(<StatusMenu game={game} onChange={onChange} />))
  return { onChange, user: userEvent.setup() }
}

describe('StatusMenu', () => {
  afterEach(() => vi.restoreAllMocks())

  it("offers a game with an ending its statuses, with the settings' words", async () => {
    const { onChange, user } = setup({ override: null, override_at: null, kind: 'ending' })
    await user.click(screen.getByRole('button', { name: /Mark as/ }))
    const items = screen.getAllByRole('menuitemradio').map((b) => b.querySelector('.font-semibold')?.textContent)
    expect(items).toEqual(['Beat it', 'Shelved', 'Abandoned', 'Not for me'])
    expect(await screen.findByText(/after a year untouched/)).toBeTruthy()
    expect(screen.queryByRole('menuitem', { name: /Clear status/ })).toBeNull()

    await user.click(screen.getByRole('menuitemradio', { name: /Beat it/ }))
    expect(onChange).toHaveBeenCalledWith('done', undefined)
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('offers an endless game its own statuses', async () => {
    const { user } = setup({ override: null, override_at: null, kind: 'endless' })
    await user.click(screen.getByRole('button', { name: /Mark as/ }))
    const items = screen.getAllByRole('menuitemradio').map((b) => b.querySelector('.font-semibold')?.textContent)
    expect(items).toEqual(['Still playing', 'Burnt out', 'Not for me'])
  })

  it('asks why for Shelved, and sends the reasons and the note', async () => {
    const { onChange, user } = setup({ override: null, override_at: null, kind: 'ending' })
    await user.click(screen.getByRole('button', { name: /Mark as/ }))
    await user.click(screen.getByRole('menuitemradio', { name: /Shelved/ }))
    const why = screen.getByRole('dialog', { name: 'Why you put it down' })
    expect(why).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement).disabled).toBe(true)

    await user.click(await screen.findByRole('button', { name: 'Too grindy' }))
    expect(screen.getByRole('button', { name: 'Too grindy' }).getAttribute('aria-pressed')).toBe('true')
    await user.type(screen.getByRole('textbox', { name: /in your own words/ }), ' the late game drags ')
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(onChange).toHaveBeenCalledWith('shelved', { codes: ['grind'], note: 'the late game drags' })
  })

  it('lets the why step be skipped, or left with Esc', async () => {
    const { onChange, user } = setup({ override: null, override_at: null, kind: 'endless' })
    await user.click(screen.getByRole('button', { name: /Mark as/ }))
    await user.click(screen.getByRole('menuitemradio', { name: /Burnt out/ }))
    expect(screen.getByRole('dialog', { name: 'Why you burnt out' })).toBeTruthy()
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(onChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole('button', { name: /Mark as/ }))
    await user.click(screen.getByRole('menuitemradio', { name: /Burnt out/ }))
    await user.click(screen.getByRole('button', { name: 'Skip' }))
    expect(onChange).toHaveBeenCalledWith('burnt_out', undefined)
  })

  it('keeps the focus on its button when a status is picked and Mark as… becomes Change', async () => {
    vi.spyOn(api, 'settings').mockResolvedValue({ abandon_after_days: 365, burnout_days: 90 } as Awaited<ReturnType<typeof api.settings>>)
    const onChange = vi.fn()
    const game = { override: null, override_at: null, kind: 'ending' } as Pick<GameView, 'override' | 'override_at' | 'kind'>
    const { rerender } = render(wrap(<StatusMenu game={game} onChange={onChange} />))
    const user = userEvent.setup()
    const button = screen.getByRole('button', { name: /Mark as/ })
    await user.click(button)
    await user.click(screen.getByRole('menuitemradio', { name: /Beat it/ }))
    expect(onChange).toHaveBeenCalledWith('done', undefined)
    expect(document.activeElement).toBe(button)
    // The status arrives: the same button now says Change, and still has the focus.
    rerender(wrap(<StatusMenu game={{ ...game, override: 'done', override_at: Date.now() / 1000 }} onChange={onChange} />))
    expect(screen.getByRole('button', { name: 'Change' })).toBe(button)
    expect(document.activeElement).toBe(button)
  })

  it('shows a set status with Change, and offers Clear', async () => {
    const { onChange, user } = setup({ override: 'done', override_at: Date.now() / 1000, kind: 'ending' })
    expect(screen.getByText('Beat it')).toBeTruthy()
    expect(screen.getByText('since today')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Change' }))
    expect(screen.getByRole('menuitemradio', { name: /Beat it/ }).getAttribute('aria-checked')).toBe('true')
    await user.click(screen.getByRole('menuitem', { name: /Clear status/ }))
    expect(onChange).toHaveBeenCalledWith(null, undefined)
  })
})
