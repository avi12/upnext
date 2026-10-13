import { afterEach, describe, expect, it, vi } from 'vitest'
import { costText, usdText } from './money'

afterEach(() => vi.restoreAllMocks())

describe('money', () => {
  it('always marks dollars as US dollars', () => {
    expect(usdText(0)).toBe('US$0')
    expect(usdText(0.0004)).toBe('under US$0.001')
    expect(usdText(0.0042)).toBe('US$0.004')
    expect(usdText(12.4)).toMatch(/^US\$12\.40$/)
  })

  it('writes every amount the same way in a comma-decimal locale (B-WEB-5)', () => {
    const own = Number.prototype.toLocaleString
    vi.spyOn(Number.prototype, 'toLocaleString').mockImplementation(function (this: number, locales, options) {
      return own.call(this, locales ?? 'de-DE', options)
    })
    expect(usdText(0.0042)).toBe('US$0,004')
    expect(usdText(0.09)).toBe('US$0,09')
    expect(usdText(0.0004)).toBe('under US$0,001')
  })

  it('writes an amount that rounds to a cent in cents', () => {
    expect(usdText(0.0096)).toBe('US$0.01')
    expect(usdText(0.0094)).toBe('US$0.009')
  })

  it('splits what was charged from what is shown at API prices', () => {
    expect(costText({ cost_usd: null, api_usd: null })).toBeNull()
    expect(costText({ cost_usd: 0.09, api_usd: 0.09 })).toBe('US$0.09')
    expect(costText({ cost_usd: null, api_usd: 0.09 })).toBe('US$0.09 at API prices')
    expect(costText({ cost_usd: 0.05, api_usd: 0.09 })).toBe('US$0.05, US$0.04 at API prices')
  })
})
