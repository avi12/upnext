import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { askedInAddress } from './bigscreen'

describe('big screen', () => {
  // The module reads the address and storage once, as it loads: each test
  // that imports it loads it afresh, whatever ran before (the static import
  // above loaded it with the address the file started with).
  beforeEach(() => {
    vi.resetModules()
  })
  afterEach(() => {
    localStorage.clear()
    sessionStorage.clear()
    window.history.replaceState(null, '', '/')
  })

  it('reads ?big= in the address', () => {
    expect(askedInAddress('?big=1')).toBe(true)
    expect(askedInAddress('?big=true')).toBe(true)
    expect(askedInAddress('?big=0')).toBe(false)
    expect(askedInAddress('?big=off')).toBe(false)
    expect(askedInAddress('?q=hades')).toBeNull()
  })

  it('turns on from the address and stays on in this browser', async () => {
    window.history.replaceState(null, '', '/?big=1')
    let mod = await import('./bigscreen')
    expect(mod.isBigScreen()).toBe(true)
    window.history.replaceState(null, '', '/library')
    vi.resetModules()
    mod = await import('./bigscreen')
    expect(mod.isBigScreen()).toBe(true)
    mod.setBigScreen(false)
    vi.resetModules()
    expect((await import('./bigscreen')).isBigScreen()).toBe(false)
  })

  it('starts in a window opened in big screen and leaves the browser\'s choice alone', async () => {
    localStorage.setItem('upnext.bigscreen', '0')
    window.upnextBigScreen = true
    try {
      let mod = await import('./bigscreen')
      expect(mod.isBigScreen()).toBe(true)
      mod.setBigScreen(false)
      // A reload of the same window keeps the switch; what's remembered for
      // other windows doesn't change.
      vi.resetModules()
      mod = await import('./bigscreen')
      expect(mod.isBigScreen()).toBe(false)
      expect(localStorage.getItem('upnext.bigscreen')).toBe('0')
      mod.setBigScreen(true)
      expect(localStorage.getItem('upnext.bigscreen')).toBe('0')
    } finally {
      delete window.upnextBigScreen
      sessionStorage.clear()
    }
    vi.resetModules()
    expect((await import('./bigscreen')).isBigScreen()).toBe(false)
  })

  it('keeps its options, with defaults for any it lacks', async () => {
    const mod = await import('./bigscreen')
    mod.setBigScreenOptions({ overscan: true })
    vi.resetModules()
    localStorage.setItem('upnext.bigscreen.options', '{"buttons":"deck"}')
    const again = await import('./bigscreen')
    let seen: unknown
    const { renderHook } = await import('@testing-library/react')
    renderHook(() => {
      seen = again.useBigScreenOptions()
    })
    expect(seen).toEqual({ buttons: 'deck', overscan: false })
  })

  it('tells the app window when it goes in and out, and nothing in a browser', async () => {
    const { tellWindow } = await import('./bigscreen')
    const postMessage = vi.fn()
    window.ipc = { postMessage }
    tellWindow(true)
    expect(postMessage).not.toHaveBeenCalled()
    window.upnextApp = true
    tellWindow(true)
    tellWindow(false)
    expect(postMessage.mock.calls).toEqual([['big-screen:on'], ['big-screen:off']])
    delete window.upnextApp
    delete window.ipc
  })

  it('tells the window only of a change, so a reload keeps F11 out of full screen', async () => {
    const postMessage = vi.fn()
    window.ipc = { postMessage }
    window.upnextApp = true
    let mod = await import('./bigscreen')
    mod.tellWindow(true)
    // The page reloads, still in big screen: the window isn't told again.
    vi.resetModules()
    mod = await import('./bigscreen')
    mod.tellWindow(true)
    expect(postMessage.mock.calls).toEqual([['big-screen:on']])
    mod.tellWindow(false)
    mod.tellWindow(true)
    expect(postMessage.mock.calls).toEqual([['big-screen:on'], ['big-screen:off'], ['big-screen:on']])
    delete window.upnextApp
    delete window.ipc
  })
})
