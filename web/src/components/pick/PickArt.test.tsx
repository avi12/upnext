import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
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
      <PickArt game={MOCK_GAME} onOpen={onOpen} />
    </QueryClientProvider>,
  )
  return { onOpen, game, art: screen.getByRole('button', { name: 'Open Stardew Valley' }) }
}

/** Screenshots don't load in a test page, so the test says they did. */
function loadShots() {
  document.querySelectorAll('img[data-shot]').forEach((img) => fireEvent.load(img))
}

/** Moves the mouse from one element to another. user-event leaves the
 *  related element out, so React would take the move for a mouse leaving
 *  the whole card. */
function moveMouse(from: Element, to: Element) {
  fireEvent.pointerOut(from, { relatedTarget: to, pointerType: 'mouse' })
  fireEvent.pointerOver(to, { relatedTarget: from, pointerType: 'mouse' })
}

/** Which screenshot the frame shows, or null for the game's own art. */
function shown(art: HTMLElement): string | null {
  return art.querySelector<HTMLElement>('img[data-shot].opacity-100')?.dataset.shot ?? null
}

describe('A pick’s art', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('says it opens the game, and opens it on a click', async () => {
    const { onOpen, art } = mount()
    expect(art.className).toMatch(/cursor-pointer/)
    expect(art.textContent).toContain('Details ›')
    // The title is the keyboard's way to the page, as before.
    expect(art.tabIndex).toBe(-1)
    await userEvent.setup().click(art)
    expect(onOpen).toHaveBeenCalledOnce()
  })

  it('fades through the screenshots in the frame once the mouse rests on it, and stops when it leaves', async () => {
    const { onOpen, art, game } = mount()
    const user = userEvent.setup()
    await user.hover(art)
    // Nothing changes before the pointer has rested for a second.
    await new Promise((r) => setTimeout(r, 500))
    expect(screen.queryByRole('group', { name: 'Screenshots' })).toBeNull()
    expect(game).toHaveBeenCalledOnce()
    const dots = await screen.findByRole('group', { name: 'Screenshots' }, { timeout: 2000 })
    loadShots()
    // Inside the art's frame, so the buttons below stay where they are.
    expect(art.querySelectorAll('img[data-shot]')).toHaveLength(2)
    expect(shown(art)).toBe('0')
    expect(art.textContent).toContain('Farming Sim · Cozy')
    expect(screen.getByRole('button', { name: 'Screenshot 1 of 2' }).getAttribute('aria-current')).toBe('true')
    await waitFor(() => expect(shown(art)).toBe('1'), { timeout: 4000 })
    expect(dots.contains(document.activeElement)).toBe(false)
    await user.unhover(art.parentElement!)
    expect(screen.queryByRole('group', { name: 'Screenshots' })).toBeNull()
    expect(shown(art)).toBeNull()
    expect(onOpen).not.toHaveBeenCalled()
  }, 10_000)

  it('shows the screenshot a dot points at, and stays on it', async () => {
    const { art, onOpen } = mount()
    const user = userEvent.setup()
    await user.hover(art)
    await screen.findByRole('group', { name: 'Screenshots' }, { timeout: 2000 })
    loadShots()
    moveMouse(art, screen.getByRole('button', { name: 'Screenshot 2 of 2' }))
    expect(shown(art)).toBe('1')
    await new Promise((r) => setTimeout(r, 3000))
    expect(shown(art)).toBe('1')
    // A dot isn't the art: choosing one doesn't open the page.
    fireEvent.click(screen.getByRole('button', { name: 'Screenshot 1 of 2' }))
    expect(shown(art)).toBe('0')
    expect(onOpen).not.toHaveBeenCalled()
  }, 10_000)

  it('changes no picture on its own with reduced motion', async () => {
    vi.stubGlobal('matchMedia', (q: string) => ({ matches: q.includes('reduce'), media: q }))
    const { art } = mount()
    const user = userEvent.setup()
    await user.hover(art)
    await screen.findByRole('group', { name: 'Screenshots' }, { timeout: 2000 })
    loadShots()
    await new Promise((r) => setTimeout(r, 3000))
    expect(shown(art)).toBeNull()
    expect(art.querySelector('img[data-shot]')!.className).toMatch(/motion-reduce:transition-none/)
    moveMouse(art, screen.getByRole('button', { name: 'Screenshot 2 of 2' }))
    expect(shown(art)).toBe('1')
  }, 10_000)

  it('starts no preview for a touch, which still just opens the page', async () => {
    const { art, game } = mount()
    fireEvent.pointerEnter(art.parentElement!, { pointerType: 'touch' })
    await new Promise((r) => setTimeout(r, 1200))
    expect(screen.queryByRole('group', { name: 'Screenshots' })).toBeNull()
    expect(game).not.toHaveBeenCalled()
  })

  it('has a Preview button for the keyboard, with the arrow keys to step and Esc to end it', async () => {
    const { art } = mount()
    const user = userEvent.setup()
    await user.tab()
    const button = screen.getByRole('button', { name: 'Preview' })
    expect(document.activeElement).toBe(button)
    await user.keyboard('{Enter}')
    expect(button.getAttribute('aria-pressed')).toBe('true')
    await screen.findByRole('group', { name: 'Screenshots' })
    loadShots()
    await user.keyboard('{ArrowRight}')
    expect(shown(art)).toBe('1')
    expect(screen.getByText('Screenshot 2 of 2')).toBeTruthy()
    await user.keyboard('{ArrowRight}')
    expect(shown(art)).toBe('0')
    await user.keyboard('{Escape}')
    expect(button.getAttribute('aria-pressed')).toBe('false')
    expect(shown(art)).toBeNull()
    expect(document.activeElement).toBe(button)
  })
})
