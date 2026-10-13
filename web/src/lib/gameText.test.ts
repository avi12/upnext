import { describe, expect, it } from 'vitest'
import { gameYear, hoursMinutes, savedText, yearOf } from './gameText'

describe('gameText', () => {
  it('takes the first year, else the year in the store date', () => {
    expect(yearOf({ first_year: 1998, release_date: '12 Nov, 2004' })).toBe(1998)
    expect(yearOf({ first_year: null, release_date: '12 Nov, 2004' })).toBe(2004)
    expect(yearOf({ first_year: null, release_date: 'Coming soon' })).toBeNull()
    expect(gameYear({ first_year: null, release_date: null })).toBe('')
  })

  it('writes hours and minutes', () => {
    expect(hoursMinutes(0.2)).toBe('1m')
    expect(hoursMinutes(45)).toBe('45m')
    expect(hoursMinutes(180)).toBe('3h')
    expect(hoursMinutes(725)).toBe('12h 5m')
  })

  it('says when a game was saved, in local days', () => {
    const now = new Date(2026, 9, 3, 20, 0) // a Saturday
    const daysAgo = (d: number) => new Date(2026, 9, 3 - d, 9, 0).getTime() / 1000
    expect(savedText(null, now)).toBe('Saved')
    expect(savedText(daysAgo(0), now)).toBe('Saved today')
    expect(savedText(daysAgo(1), now)).toBe('Saved yesterday')
    expect(savedText(daysAgo(4), now)).toBe('Saved Tuesday')
    expect(savedText(daysAgo(8), now)).toBe('Saved last week')
    expect(savedText(daysAgo(21), now)).toBe('Saved 3 weeks ago')
    expect(savedText(daysAgo(40), now)).toBe('Saved last month')
    expect(savedText(new Date(2026, 1, 10).getTime() / 1000, now)).toMatch(/^Saved in February$/)
    expect(savedText(new Date(2024, 1, 10).getTime() / 1000, now)).toMatch(/^Saved in February 2024$/)
  })
})
