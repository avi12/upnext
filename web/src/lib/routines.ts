import type { Routine, RoutineBody, RoutineDay, RoutineInput, RoutinePlace, RoutineTime, RoutineWho } from '../types'
import { minutesToText, upperFirst } from './format'

/** The longest name the server takes: it's a chip on the Pick page. */
export const ROUTINE_NAME_MAX = 40
/** Routines kept at most. */
export const ROUTINES_MAX = 20

export const DAYS: { value: RoutineDay; label: string; word: string }[] = [
  { value: 'mon', label: 'Mon', word: 'Monday' },
  { value: 'tue', label: 'Tue', word: 'Tuesday' },
  { value: 'wed', label: 'Wed', word: 'Wednesday' },
  { value: 'thu', label: 'Thu', word: 'Thursday' },
  { value: 'fri', label: 'Fri', word: 'Friday' },
  { value: 'sat', label: 'Sat', word: 'Saturday' },
  { value: 'sun', label: 'Sun', word: 'Sunday' },
]

export const TIMES: { value: RoutineTime; label: string }[] = [
  { value: 'mornings', label: 'Mornings' },
  { value: 'afternoons', label: 'Afternoons' },
  { value: 'evenings', label: 'Evenings' },
  { value: 'late_nights', label: 'Late nights' },
]

export const WHO: { value: RoutineWho; label: string }[] = [
  { value: 'solo', label: 'On my own' },
  { value: 'partner', label: 'With a partner' },
  { value: 'friends', label: 'With friends' },
  { value: 'kids', label: 'With kids' },
]

export const PLACES: { value: RoutinePlace; label: string }[] = [
  { value: 'desk', label: 'At a desk' },
  { value: 'couch', label: 'On the couch' },
  { value: 'deck', label: 'On a Steam Deck' },
]

export const INPUTS: { value: RoutineInput; label: string }[] = [
  { value: 'keyboard_mouse', label: 'Keyboard and mouse' },
  { value: 'controller', label: 'A controller' },
]

/** A routine's length choices, in the words the habits use. */
export const LENGTHS: { value: number; label: string }[] = [
  { value: 30, label: 'half an hour' },
  { value: 60, label: 'an hour' },
  { value: 90, label: 'an hour and a half' },
  { value: 120, label: 'two hours' },
  { value: 180, label: 'three hours' },
  { value: 240, label: 'four hours' },
  { value: 360, label: 'six hours' },
]

const WEEKDAYS: RoutineDay[] = ['mon', 'tue', 'wed', 'thu', 'fri']
const WEEKEND: RoutineDay[] = ['sat', 'sun']

const same = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((x) => b.includes(x))

function lowerFirst(s: string | undefined): string | undefined {
  return s && s[0].toLowerCase() + s.slice(1)
}

/** "a, b and c". */
function andList(items: string[]): string {
  return items.length < 2 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`
}

/** "Weekend evenings", "Fridays", "Mornings", or "" for any time. */
export function routineWhen(r: Pick<Routine, 'days' | 'times'>): string {
  const days = (plural: boolean) => {
    const s = plural ? 's' : ''
    if (same(r.days, WEEKEND)) return `weekend${s}`
    if (same(r.days, WEEKDAYS)) return `weekday${s}`
    return andList(DAYS.filter((d) => r.days.includes(d.value)).map((d) => d.word + s))
  }
  const times = andList(TIMES.filter((t) => r.times.includes(t.value)).map((t) => t.label.toLowerCase()))
  const text = r.days.length && r.times.length ? `${days(false)} ${times}` : r.days.length ? days(true) : times
  return upperFirst(text)
}

/** The routine in a line: "Weekend evenings · with friends · at a desk · keyboard and mouse · 2h".
 *  `anyTime` stands in for a routine with no days or times; "" leaves it out. */
export function routineSummary(r: Routine, anyTime = 'Any time, chosen by hand'): string {
  const label = <T extends string>(list: { value: T; label: string }[], v: T | null | undefined) => list.find((x) => x.value === v)?.label
  const parts = [
    routineWhen(r) || anyTime,
    label(WHO, r.who)?.toLowerCase(),
    lowerFirst(label(PLACES, r.place)),
    r.input === 'controller' ? 'on a controller' : r.input === 'keyboard_mouse' ? 'keyboard and mouse' : undefined,
    r.minutes ? minutesToText(r.minutes) : undefined,
  ]
  return parts.filter(Boolean).join(' · ')
}

/** A routine as the form edits it; a new one starts empty. */
export function routineBody(r?: Routine | null): RoutineBody {
  return {
    name: r?.name ?? '',
    days: r?.days ?? [],
    times: r?.times ?? [],
    who: r?.who ?? null,
    place: r?.place ?? null,
    input: r?.input ?? null,
    minutes: r?.minutes ?? null,
    note: r?.note ?? null,
  }
}

/** The query that holds `GET /api/routines`, shared by the Profile and the Pick page. */
export const ROUTINES_KEY = ['routines'] as const

/** Which routine the next Curate is for: one by id, none, or (undefined) the one that fits now. */
export type RoutineChoice = number | null | undefined

/** The routine a choice comes to, given what the server says fits now. */
export function chosenRoutine(routines: Routine[], now: number | null | undefined, choice: RoutineChoice): Routine | null {
  const id = choice === undefined ? now : choice
  return routines.find((r) => r.id === id) ?? null
}
