import { afterEach, describe, expect, it, vi } from 'vitest'
import { frameOs, markFrame, resizeEdges, thumbFor, windowAction } from './frame'

describe('frame', () => {
  afterEach(() => {
    delete window.upnextFrame
    delete window.ipc
    delete document.documentElement.dataset.frame
  })

  it('knows the frameless window only by what the window injects', () => {
    expect(frameOs()).toBeNull()
    window.upnextFrame = {}
    expect(frameOs()).toBeNull()
    window.upnextFrame = { os: 'windows' }
    expect(frameOs()).toBe('windows')
    window.upnextFrame = { os: 'linux' }
    expect(frameOs()).toBe('linux')
  })

  it('marks the page for the title bar only in the frameless window', () => {
    markFrame()
    expect(document.documentElement.dataset.frame).toBeUndefined()
    window.upnextFrame = { os: 'linux' }
    markFrame()
    expect(document.documentElement.dataset.frame).toBe('linux')
  })

  it('sends window actions through the ipc channel, and nothing without one', () => {
    expect(() => windowAction('close')).not.toThrow()
    const postMessage = vi.fn()
    window.ipc = { postMessage }
    windowAction('resize:sw')
    expect(postMessage).toHaveBeenCalledWith('window:resize:sw')
  })

  it('leaves the left, right and bottom edges to Windows', () => {
    expect(resizeEdges('windows')).toEqual(['n', 'ne', 'nw'])
    expect(resizeEdges('linux')).toHaveLength(8)
  })
})

describe('the Windows app window page scrollbar', () => {
  it('has no thumb when the page fits the window', () => {
    expect(thumbFor(0, 800, 800, 32)).toBeNull()
    expect(thumbFor(0, 500, 800, 32)).toBeNull()
  })

  it('runs between the title bar and the bottom of the window, sized to the page', () => {
    // A page twice the window's height: half the track.
    expect(thumbFor(0, 1600, 800, 32)).toEqual({ top: 32, height: 384 })
    expect(thumbFor(800, 1600, 800, 32)).toEqual({ top: 32 + 384, height: 384 })
    expect(thumbFor(400, 1600, 800, 32)).toEqual({ top: 32 + 192, height: 384 })
    // Overscrolled either way, it stays on the track.
    expect(thumbFor(-50, 1600, 800, 32)?.top).toBe(32)
    expect(thumbFor(5000, 1600, 800, 32)?.top).toBe(32 + 384)
  })

  it('keeps a thumb big enough to grab on a very long page', () => {
    expect(thumbFor(0, 200_000, 800, 32)?.height).toBe(32)
    expect(thumbFor(199_200, 200_000, 800, 32)).toEqual({ top: 800 - 32, height: 32 })
  })
})
