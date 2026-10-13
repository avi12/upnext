import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { codeInHash } from './pairing'

describe('the pairing code in the address', () => {
  // The module reads the address once, as it loads: each test that imports
  // it loads it afresh, whatever ran before (the static import above loaded
  // it with the address the file started with).
  beforeEach(() => {
    vi.resetModules()
  })
  afterEach(() => {
    window.history.replaceState(null, '', '/')
  })

  it('reads six digits after #pair=, and nothing else', () => {
    expect(codeInHash('#pair=482913')).toBe('482913')
    expect(codeInHash('#pair=000042')).toBe('000042')
    expect(codeInHash('#pair=48291')).toBeNull()
    expect(codeInHash('#pair=4829134')).toBeNull()
    expect(codeInHash('#pair=abcdef')).toBeNull()
    expect(codeInHash('')).toBeNull()
  })

  it('keeps the code and takes it out of the address as the UI loads', async () => {
    window.history.replaceState(null, '', '/library?q=hades#pair=482913')
    const { pairCodeFromAddress } = await import('./pairing')
    expect(pairCodeFromAddress()).toBe('482913')
    expect(window.location.pathname + window.location.search + window.location.hash).toBe('/library?q=hades')
  })

  it('leaves any other fragment alone', async () => {
    window.history.replaceState(null, '', '/#top')
    const { pairCodeFromAddress } = await import('./pairing')
    expect(pairCodeFromAddress()).toBeNull()
    expect(window.location.hash).toBe('#top')
  })
})
