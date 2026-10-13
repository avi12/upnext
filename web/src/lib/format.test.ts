import { describe, expect, it } from 'vitest'
import {
  compactNumber,
  cx,
  durationText,
  libraryStores,
  hoursToText,
  lineupTitle,
  linuxClass,
  linuxLabel,
  minutesToText,
  monthYear,
  partOfDay,
  relativeTime,
  storeName,
  toneVar,
} from './format'

describe('format', () => {
  it('names the month of a shared data date', () => {
    expect(monthYear('2026-10-01')).toBe('October 2026')
    expect(monthYear('2026-01-31')).toBe('January 2026')
    expect(monthYear(null)).toBe('')
    expect(monthYear('soon')).toBe('soon')
  })

  it('writes minutes of play as m or h', () => {
    expect(minutesToText(0)).toBe('0m')
    expect(minutesToText(-5)).toBe('0m')
    expect(minutesToText(45)).toBe('45m')
    expect(minutesToText(60)).toBe('1h')
    expect(minutesToText(90)).toBe('1.5h')
    expect(minutesToText(599)).toBe('10h')
    expect(minutesToText(600)).toBe('10h')
    expect(minutesToText(754)).toBe('13h')
  })

  it('writes hours, or nothing when unknown', () => {
    expect(hoursToText(null)).toBeNull()
    expect(hoursToText(undefined)).toBeNull()
    expect(hoursToText(0)).toBeNull()
    expect(hoursToText(0.5)).toBe('30m')
    expect(hoursToText(2)).toBe('2h')
    expect(hoursToText(2.25)).toBe('2.3h')
    expect(hoursToText(42.6)).toBe('43h')
    // Minutes that round to 60 are an hour (B-WEB-5).
    expect(hoursToText(0.999)).toBe('1h')
    expect(hoursToText(0.99)).toBe('59m')
  })

  it('says how long ago', () => {
    const now = Date.now() / 1000
    expect(relativeTime(null)).toBe('never')
    expect(relativeTime(0)).toBe('never')
    expect(relativeTime(now - 10)).toBe('just now')
    expect(relativeTime(now - 125)).toBe('2m ago')
    expect(relativeTime(now - 3 * 3600)).toBe('3h ago')
    expect(relativeTime(now - 5 * 86400)).toBe('5d ago')
  })

  it('writes durations', () => {
    expect(durationText(250)).toBe('250ms')
    expect(durationText(1500)).toBe('1.5s')
    expect(durationText(125_000)).toBe('2m 5s')
    // Rounded before it's split (B-WEB-5): never "60.0s", "1m 60s" or "1000ms".
    expect(durationText(59_940)).toBe('59.9s')
    expect(durationText(59_990)).toBe('1m 0s')
    expect(durationText(119_600)).toBe('2m 0s')
    expect(durationText(999.7)).toBe('1.0s')
  })

  it('compacts numbers and joins classes', () => {
    expect(compactNumber(null)).toBe('')
    expect(compactNumber(1500)).toMatch(/^1\.5\s?K$/)
    expect(cx('a', false, null, undefined, 'b')).toBe('a b')
  })

  it('names the lineup and the part of the day by the clock', () => {
    const at = (h: number) => new Date(2026, 9, 3, h, 30)
    expect(lineupTitle(at(4))).toBe('Late-night lineup')
    expect(lineupTitle(at(5))).toBe("This morning's lineup")
    expect(lineupTitle(at(12))).toBe("This afternoon's lineup")
    expect(lineupTitle(at(17))).toBe("Tonight's lineup")
    expect(lineupTitle(at(23))).toBe('Late-night lineup')
    expect(partOfDay(at(9))).toBe('this morning')
    expect(partOfDay(at(20))).toBe('tonight')
    expect(partOfDay(at(1))).toBe('late night')
  })

  it('labels Linux support and stores', () => {
    expect(linuxLabel(null)).toBeNull()
    expect(linuxLabel('gold')).toBe('Gold')
    expect(linuxClass('native')).toBe('text-good')
    expect(linuxClass('bronze')).toBe('text-warn')
    expect(linuxClass('borked')).toBe('text-bad')
    expect(linuxClass(undefined)).toBe('text-dim')
    expect(storeName('gog')).toBe('GOG')
    expect(storeName('steam')).toBe('Steam')
    expect(storeName('itch')).toBe('itch.io')
    expect(storeName('origin')).toBe('EA app')
    expect(storeName('xboxone')).toBe('Xbox')
    expect(storeName('newstore')).toBe('Newstore')
  })

  it('lists the stores a library has games from, Galaxy\'s too', () => {
    const counts = { steam: 1207, gog: 350, epic: 347, amazon: 0, xboxone: 37, origin: 49, uplay: 0 }
    expect(libraryStores(counts)).toEqual(['steam', 'gog', 'epic', 'origin', 'xboxone'])
    expect(libraryStores(counts, true)).toEqual(['steam', 'gog', 'epic', 'amazon', 'origin', 'xboxone'])
    expect(libraryStores(undefined, true)).toEqual(['steam', 'gog', 'epic', 'amazon'])
  })

  it('picks one of eight tones for a cover', () => {
    expect(toneVar(9)).toBe('var(--tone-1)')
    expect(toneVar(-3)).toBe('var(--tone-3)')
  })
})
