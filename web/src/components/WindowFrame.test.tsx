import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { useWindowDrag } from '../lib/windowDrag'
import { WindowControls, WindowFrame } from './WindowFrame'

const sent = vi.fn()

function inFrame(os: string) {
  window.upnextFrame = { os }
  window.ipc = { postMessage: sent }
}

function tell(maximized: boolean, focused = true, fullscreen = false) {
  act(() => {
    window.dispatchEvent(new CustomEvent('upnext:window', { detail: { maximized, focused, fullscreen } }))
  })
}

const edges = () => Array.from(document.querySelectorAll<HTMLElement>('[data-edge]')).map((e) => e.dataset.edge)

/** A top bar as Shell's: a link, a field and a button, room between them,
 *  and the window's buttons at its end. */
function TopBar() {
  const drag = useWindowDrag()
  return (
    <header data-testid="top-bar" {...drag}>
      <a href="/">UpNext home</a>
      <span data-testid="room">room</span>
      <input aria-label="Find a game" />
      <button type="button">Settings</button>
      <WindowControls />
    </header>
  )
}

/** The page with the app's top bar, and the frame as main.tsx renders it. */
function App({ bar = true }: { bar?: boolean }) {
  return (
    <>
      {bar && <TopBar />}
      <WindowFrame />
    </>
  )
}

describe('the desktop app window frame', () => {
  beforeEach(() => sent.mockClear())
  afterEach(() => {
    delete window.upnextFrame
    delete window.ipc
  })

  it('shows no window buttons in a browser, or in an app window that has the system title bar', () => {
    render(<App />)
    expect(screen.queryByTestId('window-controls')).toBeNull()
    expect(screen.queryByTestId('title-bar')).toBeNull()
    // The top bar doesn't move a window that isn't there.
    fireEvent.mouseDown(screen.getByTestId('room'), { button: 0, clientX: 300, clientY: 10 })
    fireEvent.mouseMove(window, { buttons: 1, clientX: 340, clientY: 10 })
    fireEvent.doubleClick(screen.getByTestId('room'), { button: 0 })
    // `upnextApp` alone is any app window; only the frameless one has `upnextFrame`.
    window.upnextApp = true
    render(<App />)
    expect(screen.queryByTestId('window-controls')).toBeNull()
    delete window.upnextApp
    // A system the UI doesn't know draws nothing either.
    inFrame('macos')
    render(<App />)
    expect(screen.queryByTestId('window-controls')).toBeNull()
    expect(sent).not.toHaveBeenCalled()
  })

  it('puts the window buttons at the end of the top bar, with no title bar above it', () => {
    inFrame('windows')
    render(<App />)
    expect(sent).toHaveBeenCalledWith('window:framed')
    expect(screen.queryByTestId('title-bar')).toBeNull()
    expect(document.documentElement.dataset.titlebar).toBeUndefined()
    // The bar ends with their room; the buttons float over it in a layer
    // of their own, above any panel or dialog.
    expect(screen.getByTestId('top-bar').lastElementChild).toBe(screen.getByTestId('window-controls-room'))
    const controls = screen.getByTestId('window-controls')
    expect(controls.parentElement).toBe(document.body)
    expect(controls.className).toContain('z-[1000]')
    expect(screen.getAllByRole('button', { name: 'Close' })).toHaveLength(1)
    // The app's name is the top bar's, once.
    expect(controls.textContent).toBe('')
    fireEvent.click(screen.getByRole('button', { name: 'Minimize' }))
    expect(sent).toHaveBeenLastCalledWith('window:minimize')
    fireEvent.click(screen.getByRole('button', { name: 'Maximize' }))
    expect(sent).toHaveBeenLastCalledWith('window:maximize')
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(sent).toHaveBeenLastCalledWith('window:close')
  })

  it('gives a page with no top bar a slim bar with the buttons, and no name', () => {
    inFrame('linux')
    const { rerender } = render(<App bar={false} />)
    const bar = screen.getByTestId('title-bar')
    expect(bar.textContent).toBe('')
    expect(screen.getByRole('button', { name: 'Close' })).toBeTruthy()
    expect(document.documentElement.dataset.titlebar).toBe('')
    // The top bar comes (setup done): the slim bar goes.
    rerender(<App />)
    expect(screen.queryByTestId('title-bar')).toBeNull()
    expect(document.documentElement.dataset.titlebar).toBeUndefined()
    expect(screen.getAllByRole('button', { name: 'Close' })).toHaveLength(1)
    rerender(<App bar={false} />)
    expect(screen.getByTestId('title-bar')).toBeTruthy()
  })

  it('takes the buttons away while the window is full screen', () => {
    inFrame('linux')
    const { rerender } = render(<App />)
    tell(false, true, true)
    expect(screen.queryByTestId('window-controls')).toBeNull()
    expect(edges()).toEqual([])
    // A page with no top bar gets no slim bar either.
    rerender(<App bar={false} />)
    expect(screen.queryByTestId('title-bar')).toBeNull()
    expect(document.documentElement.dataset.titlebar).toBeUndefined()
    rerender(<App />)
    tell(false)
    expect(screen.getByTestId('window-controls')).toBeTruthy()
    expect(edges()).toHaveLength(8)
  })

  it('shows Restore while the window is maximized, from what the window says', () => {
    inFrame('linux')
    render(<App />)
    tell(true)
    expect(screen.getByRole('button', { name: 'Restore' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Maximize' })).toBeNull()
    tell(false)
    expect(screen.getByRole('button', { name: 'Maximize' })).toBeTruthy()
  })

  it('moves the window from the top bar once the pointer moves with the button down, and a double click maximizes', () => {
    inFrame('windows')
    render(<App />)
    const room = screen.getByTestId('room')
    fireEvent.mouseDown(room, { button: 0, clientX: 300, clientY: 10 })
    fireEvent.mouseMove(window, { buttons: 1, clientX: 301, clientY: 10 })
    expect(sent).not.toHaveBeenCalledWith('window:drag')
    fireEvent.mouseMove(window, { buttons: 1, clientX: 306, clientY: 12 })
    expect(sent).toHaveBeenLastCalledWith('window:drag')
    sent.mockClear()
    // A press and release with no movement isn't a drag.
    fireEvent.mouseDown(screen.getByTestId('top-bar'), { button: 0, clientX: 300, clientY: 10 })
    fireEvent.mouseUp(window)
    fireEvent.mouseMove(window, { buttons: 0, clientX: 340, clientY: 10 })
    expect(sent).not.toHaveBeenCalled()
    fireEvent.doubleClick(room, { button: 0 })
    expect(sent).toHaveBeenLastCalledWith('window:maximize')
    sent.mockClear()
    // The bar's links, fields and buttons, the window's own among them, are
    // not part of the drag region.
    for (const control of [
      screen.getByRole('link', { name: 'UpNext home' }),
      screen.getByRole('textbox', { name: 'Find a game' }),
      screen.getByRole('button', { name: 'Settings' }),
      screen.getByRole('button', { name: 'Close' }),
    ]) {
      fireEvent.mouseDown(control, { button: 0, clientX: 900, clientY: 10 })
      fireEvent.mouseMove(window, { buttons: 1, clientX: 940, clientY: 30 })
      fireEvent.doubleClick(control, { button: 0 })
    }
    expect(sent).not.toHaveBeenCalled()
  })

  it('moves the window from the slim bar too', () => {
    inFrame('linux')
    render(<App bar={false} />)
    const bar = screen.getByTestId('title-bar')
    fireEvent.mouseDown(bar, { button: 0, clientX: 300, clientY: 10 })
    fireEvent.mouseMove(window, { buttons: 1, clientX: 310, clientY: 10 })
    expect(sent).toHaveBeenLastCalledWith('window:drag')
    fireEvent.doubleClick(bar, { button: 0 })
    expect(sent).toHaveBeenLastCalledWith('window:maximize')
  })

  it('resizes from the top on Windows, from every side on Linux, and not while maximized', () => {
    inFrame('windows')
    const { unmount } = render(<App />)
    expect(edges()).toEqual(['n', 'ne', 'nw'])
    fireEvent.mouseDown(document.querySelector('[data-edge="ne"]')!, { button: 0 })
    expect(sent).toHaveBeenLastCalledWith('window:resize:ne')
    unmount()
    inFrame('linux')
    render(<App />)
    expect(edges()).toEqual(['n', 'ne', 'e', 'se', 's', 'sw', 'w', 'nw'])
    tell(true)
    expect(edges()).toEqual([])
  })

  it('keeps its buttons reachable from the keyboard, with their names, and out of big screen focus', () => {
    inFrame('windows')
    render(<App />)
    expect(screen.getByTestId('window-controls').hasAttribute('data-focus-skip')).toBe(true)
    for (const name of ['Minimize', 'Maximize', 'Close']) {
      const button = screen.getByRole('button', { name })
      expect(button.tabIndex).toBe(0)
      expect(button.getAttribute('title')).toBe(name)
    }
  })
})
