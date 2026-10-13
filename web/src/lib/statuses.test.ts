import { describe, expect, it } from 'vitest'
import { DEFAULT_LINES, OVERRIDES, dayText, overrideMeta, sinceText, span } from './statuses'

describe('statuses', () => {
  it('writes lengths of days in words', () => {
    expect(span(730)).toBe('2 years')
    expect(span(365)).toBe('a year')
    expect(span(90)).toBe('3 months')
    expect(span(30)).toBe('a month')
    expect(span(14)).toBe('2 weeks')
    expect(span(7)).toBe('a week')
    expect(span(10)).toBe('10 days')
    expect(span(1)).toBe('a day')
  })

  it('offers each kind of game its own statuses', () => {
    const forKind = (k: 'ending' | 'endless') => OVERRIDES.filter((o) => o.kinds.includes(k)).map((o) => o.value)
    expect(forKind('ending')).toEqual(['done', 'shelved', 'abandoned', 'hidden'])
    expect(forKind('endless')).toEqual(['still_playing', 'burnt_out', 'hidden'])
    expect(OVERRIDES.filter((o) => o.asksWhy).map((o) => o.value)).toEqual(['shelved', 'abandoned', 'burnt_out'])
  })

  it('puts the settings into the hints', () => {
    expect(overrideMeta('shelved')?.hint(DEFAULT_LINES)).toContain('after 2 years untouched')
    expect(overrideMeta('burnt_out')?.hint({ abandon_after_days: 730, burnout_days: 14 })).toContain('Out of picks for 2 weeks')
    expect(overrideMeta('done')?.label).toBe('Beat it')
    expect(overrideMeta(null)).toBeNull()
  })

  it('says since when', () => {
    expect(sinceText(null)).toBeNull()
    expect(sinceText(Date.now() / 1000)).toBe('since today')
    expect(dayText(new Date(2020, 0, 5).getTime() / 1000)).toMatch(/2020/)
  })
})
