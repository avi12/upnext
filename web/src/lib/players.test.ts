import { afterEach, describe, expect, it } from 'vitest'
import type { Player, PlayersResponse } from '../types'
import {
  everyones,
  memory,
  nudgeOnce,
  pickerNeeded,
  pinOk,
  placeholderName,
  possessive,
  remember,
  setAskEachTime,
  stillSeenText,
  type Memory,
} from './players'

function player(id: number, name: string, has_pin = false): Player {
  return { id, name, colour: '#4f8cff', has_pin, created_at: 1790000000, default: id === 1 }
}

const two: PlayersResponse = { players: [player(1, 'Alex'), player(2, 'Sam')], current: 1, bound: false }
const chose = (last: number | null, extra: Partial<Memory> = {}): Memory => ({ last, ask: false, chosenHere: false, ...extra })

describe('pickerNeeded', () => {
  it('stays away with one player, and on a device bound to one', () => {
    expect(pickerNeeded({ ...two, players: [player(1, 'Alex')] }, chose(null))).toBe(false)
    expect(pickerNeeded({ ...two, bound: true }, chose(null))).toBe(false)
    expect(pickerNeeded(undefined, chose(null))).toBe(false)
  })

  it('shows with two players until this browser has chosen', () => {
    expect(pickerNeeded(two, chose(null))).toBe(true)
    expect(pickerNeeded(two, chose(1))).toBe(false)
  })

  it('shows again when the server answers for someone else than the one chosen (a cleared cookie)', () => {
    expect(pickerNeeded(two, chose(2))).toBe(true)
  })

  it('shows when nobody is chosen, even with one player (the default player has a PIN)', () => {
    expect(pickerNeeded({ ...two, current: null }, chose(1))).toBe(true)
    expect(pickerNeeded({ players: [player(1, 'Alex', true)], current: null, bound: false }, chose(null))).toBe(true)
  })

  it('asks each time a window opens, once per window, with Ask each time on', () => {
    expect(pickerNeeded(two, chose(1, { ask: true }))).toBe(true)
    expect(pickerNeeded(two, chose(1, { ask: true, chosenHere: true }))).toBe(false)
  })
})

describe('what the browser remembers', () => {
  afterEach(() => {
    localStorage.clear()
    sessionStorage.clear()
  })

  it('the player chosen, in this window, and Ask each time off in a browser', () => {
    expect(memory()).toEqual({ last: null, ask: false, chosenHere: false })
    remember(2)
    setAskEachTime(true)
    expect(memory()).toEqual({ last: 2, ask: true, chosenHere: true })
  })
})

describe('words', () => {
  it('names owners, PINs and who still sees a hidden game', () => {
    expect(possessive('Sam')).toBe('Sam’s')
    expect(possessive('Chris')).toBe('Chris’')
    expect(pinOk('1234') && pinOk('12345678')).toBe(true)
    expect(pinOk('123') || pinOk('123456789') || pinOk('12a4')).toBe(false)
    expect(stillSeenText('others', { still_seen_by: [], through_shared: [] })).toBeNull()
    expect(stillSeenText('others', { still_seen_by: ['Sam'], through_shared: [] })).toBe('Sam has their own copy, so they’ll still see it.')
    expect(stillSeenText('everyone', { still_seen_by: ['Sam', 'Kit', 'Ash'], through_shared: [] })).toBe(
      'Sam, Kit and Ash have their own copies, so they’ll still see it.',
    )
    // Through someone else's shared account, rather than a copy of their own.
    expect(stillSeenText('others', { still_seen_by: ['Sam', 'Kit'], through_shared: ['Kit'] })).toBe(
      'Sam has their own copy, so they’ll still see it. Kit still sees it through someone else’s shared account.',
    )
    // Hidden only from you: the others were never meant to lose it.
    expect(stillSeenText('me', { still_seen_by: ['Sam'], through_shared: [] })).toBeNull()
  })
})

describe('everyones', () => {
  it('marks a setting in You as everyone’s only when several people play here', () => {
    expect(everyones(false)).toBeUndefined()
    expect(everyones(false, 'optional')).toBe('optional')
    expect(everyones(true)).toBe('everyone’s')
    expect(everyones(true, 'optional')).toBe('optional · everyone’s')
  })
})

describe('names UpNext made up', () => {
  afterEach(() => localStorage.clear())

  it('are "Player N", as the server makes them', () => {
    for (const made of ['Player 1', 'Player 12', 'Player 2 2']) expect(placeholderName(made), made).toBe(true)
    for (const chosen of ['Sam', 'Player', 'Player One', 'player 1', 'Player 1 again', 'Kit 1']) expect(placeholderName(chosen), chosen).toBe(false)
  })

  it('are pointed out once per player in a browser', () => {
    expect([nudgeOnce(1), nudgeOnce(1), nudgeOnce(2)]).toEqual([true, false, true])
    expect(localStorage.getItem('upnext.player.nudged')).toBe('1,2')
  })
})
