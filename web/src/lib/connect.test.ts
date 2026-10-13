import { afterEach, describe, expect, it, vi } from 'vitest'
import { canSwitch, deviceKindText, missedComputer, openComputer, remoteName } from './connect'

describe('the app window on another computer', () => {
  afterEach(() => {
    delete window.upnextApp
    delete window.upnextRemote
    delete window.ipc
  })

  it('knows which computer it shows, and only the app window can switch', () => {
    expect(remoteName()).toBeNull()
    expect(canSwitch()).toBe(false)
    window.upnextRemote = { name: 'den' }
    expect(remoteName()).toBe('den')
    window.upnextRemote = { name: '' }
    expect(remoteName()).toBeNull()

    const sent = vi.fn()
    window.ipc = { postMessage: sent }
    expect(canSwitch()).toBe(false)
    window.upnextApp = true
    expect(canSwitch()).toBe(true)
    openComputer('4f2a9c01d3e85b67')
    openComputer(null)
    expect(sent.mock.calls).toEqual([['connect:4f2a9c01d3e85b67'], ['connect:here']])
  })

  it('reads the computer the window did not open from the address, and why, and nothing odd', () => {
    expect(missedComputer('?unreachable=4f2a9c01d3e85b67')).toEqual({ id: '4f2a9c01d3e85b67', changed: false })
    expect(missedComputer('?changed=4f2a9c01d3e85b67')).toEqual({ id: '4f2a9c01d3e85b67', changed: true })
    expect(missedComputer('?unreachable=../x')).toBeNull()
    expect(missedComputer('')).toBeNull()
  })

  it('names the kind of device asking', () => {
    expect(deviceKindText('phone')).toBe('A phone')
    expect(deviceKindText('tv')).toBe('A TV')
    expect(deviceKindText('something new')).toBe('A computer')
  })
})
