import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { renderHook } from '@testing-library/react'
import { GLYPHS, PadReader, REPEAT_AFTER, REPEAT_EVERY, SETTLE, held, padStyle, type PadAction, type PadState } from './pad'
import { lastInput, useGamepad } from './useGamepad'

/** A standard-mapping pad with nothing pressed. */
function pad(id = 'Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)', index = 0): PadState & {
  press: (...buttons: number[]) => void
  release: () => void
  tilt: (x: number, y: number) => void
  pull: (button: number, value: number) => void
} {
  const buttons: { pressed: boolean; value: number }[] = Array.from({ length: 17 }, () => ({ pressed: false, value: 0 }))
  const axes = [0, 0, 0, 0]
  const state = {
    id,
    index,
    connected: true,
    mapping: 'standard',
    buttons,
    axes,
    press: (...bs: number[]) => bs.forEach((b) => (buttons[b] = { pressed: true, value: 1 })),
    release: () => {
      buttons.fill({ pressed: false, value: 0 })
      axes.fill(0)
    },
    tilt: (x: number, y: number) => {
      axes[0] = x
      axes[1] = y
    },
    pull: (b: number, value: number) => (buttons[b] = { pressed: false, value }),
  }
  return state
}

describe('the button mapping', () => {
  it('maps the standard buttons onto big screen actions', () => {
    const p = pad()
    const names: Record<number, PadAction> = { 0: 'accept', 1: 'back', 2: 'menu', 3: 'request', 4: 'prevTab', 5: 'nextTab', 6: 'pageUp', 7: 'pageDown', 8: 'help', 9: 'settings', 12: 'up', 13: 'down', 14: 'left', 15: 'right' }
    for (const [i, action] of Object.entries(names)) {
      p.release()
      p.press(Number(i))
      expect([...held(p)]).toEqual([action])
    }
  })

  it('reads a trigger pulled halfway as pressed, and the stick along its stronger axis', () => {
    const p = pad()
    p.pull(7, 0.8)
    expect(held(p).has('pageDown')).toBe(true)
    p.release()
    p.tilt(0.3, -0.9)
    expect([...held(p)]).toEqual(['up'])
    p.tilt(0.7, 0.2)
    expect([...held(p)]).toEqual(['right'])
    p.tilt(0.2, 0.2)
    expect(held(p).size).toBe(0)
  })

  it('names the buttons after the pad', () => {
    expect(padStyle('Xbox 360 Controller (XInput STANDARD GAMEPAD)')).toBe('xbox')
    expect(padStyle('Sony Interactive Entertainment Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)')).toBe('playstation')
    expect(padStyle('054c-0ce6-DualSense Wireless Controller')).toBe('playstation')
    expect(padStyle('Steam Deck (STANDARD GAMEPAD Vendor: 28de Product: 1205)')).toBe('deck')
    expect(padStyle('28de-1205-Steam Deck')).toBe('deck')
    expect(GLYPHS.xbox.accept).toBe('A')
    expect(GLYPHS.playstation.accept).toBe('✕')
    expect(GLYPHS.deck.prevTab).toBe('L1')
  })
})

describe('PadReader', () => {
  it('acts once per press and not again while the button is held', () => {
    const r = new PadReader()
    const p = pad()
    p.press(0)
    expect(r.poll([p], 0).actions).toEqual(['accept'])
    expect(r.poll([p], 1000).actions).toEqual([])
    p.release()
    expect(r.poll([p], 1016).actions).toEqual([])
    p.press(0)
    expect(r.poll([p], 1032).actions).toEqual(['accept'])
  })

  it('repeats a held direction after the delay, then at the repeat rate', () => {
    const r = new PadReader()
    const p = pad()
    p.press(13)
    expect(r.poll([p], 0).actions).toEqual(['down'])
    expect(r.poll([p], REPEAT_AFTER - 1).actions).toEqual([])
    expect(r.poll([p], REPEAT_AFTER).actions).toEqual(['down'])
    expect(r.poll([p], REPEAT_AFTER + REPEAT_EVERY - 1).actions).toEqual([])
    expect(r.poll([p], REPEAT_AFTER + REPEAT_EVERY).actions).toEqual(['down'])
  })

  it('reads the stick once it has been at its centre, and the other way only after it rests there', () => {
    const r = new PadReader()
    const p = pad()
    expect(r.poll([p], 0).actions).toEqual([])
    p.tilt(0, -0.9)
    expect(r.poll([p], 16).actions).toEqual(['up'])
    // Let go and pushed the same way again at once: it still counts.
    p.tilt(0, 0)
    expect(r.poll([p], 32).actions).toEqual([])
    p.tilt(0, -0.9)
    expect(r.poll([p], 48).actions).toEqual(['up'])
    // Flicked across in a frame or two: not until it has rested at the centre.
    p.tilt(0, 0.1)
    expect(r.poll([p], 64).actions).toEqual([])
    p.tilt(0, 0.9)
    expect(r.poll([p], 80).actions).toEqual([])
    p.tilt(0, 0)
    expect(r.poll([p], 96).actions).toEqual([])
    expect(r.poll([p], 96 + SETTLE).actions).toEqual([])
    p.tilt(0, 0.9)
    expect(r.poll([p], 112 + SETTLE).actions).toEqual(['down'])
  })

  it('never reads a trigger the browser put on a stick axis as the stick held up', () => {
    // WebKitGTK with Microsoft's wireless adapter on Linux: RT moves the left
    // stick's vertical axis from -1, let go, to 1, pulled, and the axis
    // rests at -1 once it has been pulled.
    const r = new PadReader()
    const p = pad()
    let t = 0
    const at = (y: number) => {
      p.tilt(0, y)
      t += 16
      return r.poll([p], t).actions
    }
    expect(at(0)).toEqual([])
    // The first pull reads as down once, which nothing here can tell apart from the stick.
    expect(at(1)).toEqual(['down'])
    // Let go, passing the centre on its way to -1, and resting there.
    expect([at(0.5), at(0), at(-0.5), at(-1)].flat()).toEqual([])
    for (let i = 0; i < 100; i++) expect(at(-1)).toEqual([])
    // Pulled and let go again: still nothing.
    expect([at(0), at(1), at(1), at(0), at(-1), at(-1)].flat()).toEqual([])
    // Let go slowly, over two seconds, so it spends a while near the centre:
    // it never stays still there, so it still doesn't read as up.
    expect(at(1)).toEqual([])
    const frames = 2000 / 16
    for (let i = 1; i <= frames; i++) expect(at(1 - (2 * i) / frames)).toEqual([])
    for (let i = 0; i < 100; i++) expect(at(-1)).toEqual([])
    // A pad that comes in with an axis already at -1 doesn't lean either.
    const fresh = new PadReader()
    const q = pad('Xbox Wireless Controller', 1)
    q.tilt(0, -1)
    expect(fresh.poll([q], 0).actions).toEqual([])
    expect(fresh.poll([q], 1000).actions).toEqual([])
    // The buttons still work.
    q.press(0)
    expect(fresh.poll([q], 1016).actions).toEqual(['accept'])
  })

  it('takes input from any connected pad and says which one sent it', () => {
    const r = new PadReader()
    const one = pad('Xbox', 0)
    const two = pad('DualSense Wireless Controller', 1)
    two.press(1)
    const out = r.poll([one, null, two], 0)
    expect(out.actions).toEqual(['back'])
    expect(out.from).toBe(two)
  })
})

describe('useGamepad', () => {
  let pads: (PadState | null)[]
  let frames: Array<(t: number) => void>
  let now: number

  /** Runs the waiting animation frame, a frame's length later. */
  const frame = () => {
    now += 16
    const waiting = frames
    frames = []
    for (const f of waiting) f(now)
  }

  beforeEach(() => {
    pads = []
    frames = []
    now = 0
    Object.defineProperty(navigator, 'getGamepads', { configurable: true, value: () => pads })
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((f) => {
      frames.push(f)
      return frames.length
    })
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('reads the pads each frame only while one is connected', () => {
    const seen: PadAction[] = []
    renderHook(() => useGamepad((a) => seen.push(a)))
    // No pad yet: one look, then it stops asking for frames.
    frame()
    expect(frames).toHaveLength(0)

    const p = pad()
    pads = [p]
    window.dispatchEvent(new Event('gamepadconnected'))
    expect(frames).toHaveLength(1)
    p.press(15)
    frame()
    expect(seen).toEqual(['right'])
    expect(lastInput()).toBe('gamepad')
    // Held: nothing more until the repeat delay has passed.
    for (let i = 0; i < 20; i++) frame()
    expect(seen).toEqual(['right'])
    for (let i = 0; i < 10; i++) frame()
    expect(seen.length).toBeGreaterThan(1)

    pads = []
    frame()
    expect(frames).toHaveLength(0)
  })
})
