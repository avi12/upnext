import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView } from '../../types'
import { MOCK_GAME } from '../../test/mockGame'
import { PickArt } from './PickArt'

const FULL: GameView = {
  ...MOCK_GAME,
  linux: 'platinum',
  screenshots: [
    { thumb: 'https://example.com/1.jpg', full: 'https://example.com/1-full.jpg' },
    { thumb: 'https://example.com/2.jpg', full: 'https://example.com/2-full.jpg' },
  ],
}

function mount() {
  const game = vi.spyOn(api, 'game').mockResolvedValue(FULL)
  const onOpen = vi.fn()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <PickArt game={MOCK_GAME} onOpen={onOpen} showLinux />
    </QueryClientProvider>,
  )
  return { onOpen, game, art: screen.getByRole('button', { name: 'Open Stardew Valley' }) }
}

describe('A pick’s art', () => {
  afterEach(() => vi.restoreAllMocks())

  it('says it opens the game, and opens it on a click', async () => {
    const { onOpen, art } = mount()
    expect(art.className).toMatch(/cursor-pointer/)
    expect(art.textContent).toContain('Details ›')
    // The title is the keyboard's way to the page, as before.
    expect(art.tabIndex).toBe(-1)
    await userEvent.setup().click(art)
    expect(onOpen).toHaveBeenCalledOnce()
  })

  it('opens a quick look when the mouse rests on it, with screenshots, the numbers and the top tags', async () => {
    const { onOpen, art } = mount()
    const user = userEvent.setup()
    await user.hover(art)
    const look = await screen.findByRole('dialog', { name: 'Quick look: Stardew Valley' }, { timeout: 2000 })
    expect(look.textContent).toContain('53h to beat')
    expect(look.textContent).toContain('98% · 600K reviews')
    expect(look.textContent).toContain('● Platinum')
    expect(screen.getByRole('list', { name: 'Top tags' }).textContent).toBe('Farming SimCozy')
    expect(await screen.findByAltText('Stardew Valley, screenshot 1 of 2')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Next screenshot' }))
    expect(screen.getByAltText('Stardew Valley, screenshot 2 of 2')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Open' }))
    expect(onOpen).toHaveBeenCalledOnce()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('opens no quick look for a touch, which still just opens the page', async () => {
    const { art } = mount()
    fireEvent.pointerOver(art, { pointerType: 'touch' })
    await new Promise((r) => setTimeout(r, 700))
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('has a Quick look button for the keyboard, and Esc closes it back to that button', async () => {
    mount()
    const user = userEvent.setup()
    await user.tab()
    const button = screen.getByRole('button', { name: 'Quick look' })
    expect(document.activeElement).toBe(button)
    await user.keyboard('{Enter}')
    expect(button.getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByRole('dialog', { name: 'Quick look: Stardew Valley' })).toBeTruthy()
    await user.tab()
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(document.activeElement).toBe(button)
  })
})
