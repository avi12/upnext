import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { renderHook } from '@testing-library/react'

// `whenFocused` remembers the focus for the page's life, so each test loads
// it afresh.
async function load() {
  vi.resetModules()
  return (await import('./focus')).whenFocused
}

describe('whenFocused', () => {
  let focused = false
  beforeEach(() => {
    focused = false
    vi.spyOn(document, 'hasFocus').mockImplementation(() => focused)
  })
  afterEach(() => {
    delete window.upnextApp
    vi.restoreAllMocks()
  })

  it('starts at once in a browser, focused or not', async () => {
    const whenFocused = await load()
    const start = vi.fn()
    whenFocused(start)
    expect(start).toHaveBeenCalledTimes(1)
  })

  it('starts at once in an app window that has the focus', async () => {
    window.upnextApp = true
    focused = true
    const whenFocused = await load()
    const start = vi.fn()
    whenFocused(start)
    expect(start).toHaveBeenCalledTimes(1)
  })

  it('waits for the focus in an app window in the background, then starts once', async () => {
    window.upnextApp = true
    const whenFocused = await load()
    const undo = vi.fn()
    const start = vi.fn(() => undo)
    const stop = whenFocused(start)
    expect(start).not.toHaveBeenCalled()
    focused = true
    window.dispatchEvent(new Event('focus'))
    // The window's own event after the page's changes nothing.
    window.dispatchEvent(new Event('upnext:focus'))
    expect(start).toHaveBeenCalledTimes(1)
    stop()
    expect(undo).toHaveBeenCalledTimes(1)
  })

  it("starts on the window's own focus event too", async () => {
    window.upnextApp = true
    const whenFocused = await load()
    const start = vi.fn()
    whenFocused(start)
    expect(start).not.toHaveBeenCalled()
    window.dispatchEvent(new Event('upnext:focus'))
    expect(start).toHaveBeenCalledTimes(1)
  })

  it('starts at once after the window has had the focus, even when it has lost it', async () => {
    window.upnextApp = true
    const whenFocused = await load()
    const first = vi.fn()
    whenFocused(first)
    expect(first).not.toHaveBeenCalled()
    window.dispatchEvent(new Event('focus'))
    const start = vi.fn()
    whenFocused(start)
    expect(start).toHaveBeenCalledTimes(1)
  })

  it('never starts when stopped while waiting', async () => {
    window.upnextApp = true
    const whenFocused = await load()
    const start = vi.fn()
    whenFocused(start)()
    window.dispatchEvent(new Event('focus'))
    expect(start).not.toHaveBeenCalled()
  })

  it("leaves the pads alone in a background app window until it has the focus (WebKitGTK's spin)", async () => {
    window.upnextApp = true
    vi.resetModules()
    const { useGamepad } = await import('./useGamepad')
    const getGamepads = vi.fn(() => [])
    Object.defineProperty(navigator, 'getGamepads', { value: getGamepads, configurable: true })
    const listen = vi.spyOn(window, 'addEventListener')
    const asked = () => getGamepads.mock.calls.length + listen.mock.calls.filter(([type]) => type === 'gamepadconnected').length
    const { unmount } = renderHook(() => useGamepad(() => {}))
    await new Promise((r) => requestAnimationFrame(r))
    expect(asked()).toBe(0)
    focused = true
    window.dispatchEvent(new Event('focus'))
    await new Promise((r) => requestAnimationFrame(r))
    expect(getGamepads).toHaveBeenCalled()
    expect(listen.mock.calls.some(([type]) => type === 'gamepadconnected')).toBe(true)
    unmount()
    delete (navigator as { getGamepads?: unknown }).getGamepads
  })
})
