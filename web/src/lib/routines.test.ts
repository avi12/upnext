import { describe, expect, it } from 'vitest'
import type { Routine } from '../types'
import { routineBody, routineSummary, routineWhen } from './routines'

const routine = (r: Partial<Routine>): Routine => ({ ...routineBody(), id: 1, name: 'R', ...r }) as Routine

describe('routines', () => {
  it('says when a routine is for', () => {
    expect(routineWhen({ days: ['sat', 'sun'], times: ['evenings'] })).toBe('Weekend evenings')
    expect(routineWhen({ days: ['mon', 'tue', 'wed', 'thu', 'fri'], times: [] })).toBe('Weekdays')
    expect(routineWhen({ days: ['fri', 'sat'], times: ['evenings', 'late_nights'] })).toBe('Friday and Saturday evenings and late nights')
    expect(routineWhen({ days: ['fri'], times: [] })).toBe('Fridays')
    expect(routineWhen({ days: [], times: ['mornings'] })).toBe('Mornings')
    expect(routineWhen({ days: [], times: [] })).toBe('')
  })

  it('sums a routine up in a line', () => {
    expect(routineSummary(routine({ days: ['fri'], times: ['evenings'], who: 'friends', place: 'desk', input: 'keyboard_mouse', minutes: 120 }))).toBe(
      'Friday evenings · with friends · at a desk · keyboard and mouse · 2h',
    )
    expect(routineSummary(routine({ place: 'deck', input: 'controller', minutes: 90 }))).toBe('Any time, chosen by hand · on a Steam Deck · on a controller · 1.5h')
  })
})
