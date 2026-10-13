import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import { BigShell } from './BigShell'

// The shell's input and window parts, which need a gamepad or the app window.
// The pad's actions are kept, so a test can press LB and RB as the pad does.
const pad: { act?: (a: string, p: { id: string }) => void } = {}
vi.mock('../../lib/gamepad/useGamepad', () => ({ useGamepad: (act: (a: string, p: { id: string }) => void) => void (pad.act = act) }))
vi.mock('../../lib/theme', () => ({ useThemeSync: () => {} }))
vi.mock('../../lib/windowDrag', () => ({ useWindowDrag: () => ({}) }))
vi.mock('../WindowFrame', () => ({ WindowControls: () => null }))

/** jsdom lays nothing out, and the focus engine only counts what has a size. */
const realRect = HTMLElement.prototype.getBoundingClientRect

describe('big screen shell', () => {
  beforeEach(() => {
    HTMLElement.prototype.getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 100, bottom: 40, x: 0, y: 0, width: 100, height: 40, toJSON: () => ({}) }) as DOMRect
    // A frame drawn as soon as the page has changed, before React's later
    // effects run: the order a slow device can see, which jsdom's timer
    // for frames never shows.
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((draw) => {
      queueMicrotask(() => draw(performance.now()))
      return 1
    })
  })
  afterEach(() => {
    vi.restoreAllMocks()
    HTMLElement.prototype.getBoundingClientRect = realRect
    delete document.documentElement.dataset.bigscreen
  })

  /** A page of its own: keyed, so each draws new elements as a real page does. */
  function Page({ first }: { first: string }) {
    return (
      <section key={first}>
        <button type="button">{first}</button>
      </section>
    )
  }

  /** A page with nothing to focus, as Lists is with nothing saved. */
  function Empty() {
    return <p>Nothing saved yet.</p>
  }

  /** A key as the browser sends it, outside Testing Library's act(), which
   *  would run every effect before the frame. */
  function press(key: string) {
    window.dispatchEvent(new KeyboardEvent('keydown', { key }))
  }

  function shell() {
    // One player: the top bar has no player switcher.
    vi.spyOn(api, 'players').mockResolvedValue({
      players: [{ id: 1, name: 'Alex', colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: true }],
      current: 1,
      bound: false,
    })
    return render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <MemoryRouter initialEntries={['/']}>
          <Routes>
            <Route element={<BigShell />}>
              <Route index element={<Page first="Tonight's lineup" />} />
              <Route path="library" element={<Page first="First cover" />} />
              <Route path="lists" element={<Page first="First list" />} />
              <Route path="versus" element={<Empty />} />
            </Route>
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    )
  }

  it('moves the focus into the new page when the bumpers change it', async () => {
    shell()
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText("Tonight's lineup")))
    // The person went up to the tabs, then pressed RB: the focus follows
    // the page rather than staying on Curate's tab.
    screen.getByRole('link', { name: 'Curate' }).focus()
    press(']')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First cover')))
    press(']')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First list')))
    // And from the page itself, LB goes back with it.
    press('[')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First cover')))
  })

  // On the Linux device test, RB onto a page with nothing to focus left
  // the focus on Curate's tab: it goes to the new page's tab.
  it('puts the focus on the tab of a page with nothing to focus', async () => {
    shell()
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText("Tonight's lineup")))
    const bumper = (a: string) => pad.act!(a, { id: 'Xbox 360 Controller (STANDARD GAMEPAD)' })
    bumper('nextTab')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First cover')))
    bumper('nextTab')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First list')))
    // From a focus on the page, which goes with it.
    bumper('nextTab')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Help me decide' })))
    // From a focus on the tab, which stays put.
    bumper('prevTab')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First list')))
    screen.getByRole('link', { name: 'Lists' }).focus()
    bumper('nextTab')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Help me decide' })))
    // And the keyboard's ] does the same.
    press('[')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('First list')))
    press(']')
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Help me decide' })))
  })

  it('leaves room around the tabs for the focus ring', () => {
    shell()
    // The strip scrolls sideways, which clips what's outside it: on every
    // side its padding is wider than the ring the shell draws and its offset.
    const root = document.documentElement.style
    const ring = parseFloat(root.getPropertyValue('--big-ring')) + parseFloat(root.getPropertyValue('--big-ring-offset'))
    expect(ring).toBeGreaterThan(0)
    const style = getComputedStyle(screen.getByRole('navigation', { name: 'Big screen' }))
    for (const side of [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft]) {
      expect(side).toMatch(/rem$/)
      expect(parseFloat(side)).toBeGreaterThan(ring)
    }
  })
})
