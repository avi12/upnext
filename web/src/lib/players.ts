// Players: who's playing in this browser, and what the
// browser remembers about it. The server knows who a request is for from
// the player cookie it sets; the browser only remembers whom it chose and
// whether to ask again, so it knows when to show "Who's playing?".

import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import type { HideAnswer, HideScope, Player, PlayersResponse } from '../types'
import { inApp } from './frame'
import { keys } from './queries'

/** The player this browser chose last. */
const LAST = 'upnext.player'
/** "Ask each time UpNext opens": '1' or '0'; unset follows `inApp()`. */
const ASK = 'upnext.player.ask'
/** Set once a player is chosen in this window or tab. */
const CHOSEN = 'upnext.player.chosen'

function read(store: () => Storage, key: string): string | null {
  try {
    return store().getItem(key)
  } catch {
    return null
  }
}

function write(store: () => Storage, key: string, value: string | null) {
  try {
    if (value == null) store().removeItem(key)
    else store().setItem(key, value)
  } catch {
    /* storage turned off: the picker asks again next time */
  }
}

const local = () => window.localStorage
const session = () => window.sessionStorage

/** What this browser remembers about who plays here. */
export interface Memory {
  /** The player chosen last, or null when none has been. */
  last: number | null
  /** Ask each time UpNext opens. */
  ask: boolean
  /** A player was chosen in this window or tab already. */
  chosenHere: boolean
}

export function memory(): Memory {
  const last = Number(read(local, LAST))
  const ask = read(local, ASK)
  return {
    last: Number.isInteger(last) && last > 0 ? last : null,
    // On by default in the app window, the shared family computer; off in
    // a browser, which usually belongs to one person.
    ask: ask == null ? inApp() : ask === '1',
    chosenHere: read(session, CHOSEN) === '1',
  }
}

export function setAskEachTime(on: boolean) {
  write(local, ASK, on ? '1' : '0')
}

/** Remember `id` as this browser's player, chosen in this window too. */
export function remember(id: number) {
  write(local, LAST, String(id))
  write(session, CHOSEN, '1')
}

/** Whether "Who's playing?" shows: with two or more players, until this
 *  browser has chosen one; again when the one it chose is gone or isn't who
 *  the server answers for (its cookie was cleared); and on each new window
 *  with Ask each time on. Never on a device bound to one player. With one
 *  player, only when nobody is chosen: their PIN is asked for. */
export function pickerNeeded(state: PlayersResponse | undefined, m: Memory): boolean {
  if (!state || state.bound || state.players.length === 0) return false
  if (state.current == null) return true
  if (state.players.length < 2) return false
  if (m.last == null || m.last !== state.current) return true
  return m.ask && !m.chosenHere
}

/** Everyone, who's playing, and whether there is more than one player. */
export function usePlayers() {
  const q = useQuery({ queryKey: keys.players, queryFn: api.players, staleTime: 60_000, retry: false })
  const players = q.data?.players ?? []
  const current = players.find((p) => p.id === q.data?.current) ?? null
  return {
    data: q.data,
    players,
    current,
    bound: q.data?.bound === true,
    /** Two or more players: the UI shows who's playing. */
    multi: players.length > 1,
    others: players.filter((p) => p.id !== current?.id),
    isLoading: q.isLoading,
  }
}

/** A number that changes each time this browser switches from one player
 *  to another, and not when the players first load. As a page's key, it
 *  gives the page fresh state for whoever plays now: the query cache starts
 *  over on a switch, but a request typed or a reason given stayed on the
 *  page and went with the next player's picks (code health B-WEB-2). */
export function usePlayerSwitches(): number {
  const id = usePlayers().current?.id ?? null
  const [seen, setSeen] = useState({ id, switches: 0 })
  // While a switch fetches the players again there is nobody (null): only
  // another player counts.
  if (id != null && id !== seen.id) setSeen({ id, switches: seen.id == null ? seen.switches : seen.switches + 1 })
  return seen.switches
}

/** Play as someone in this browser. Every answer from before was someone
 *  else's, so all of it is fetched again. */
export function useChoosePlayer(onDone?: (p: Player) => void) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, pin }: { id: number; pin?: string }) => api.choosePlayer(id, pin),
    onSuccess: (p) => {
      remember(p.id)
      onDone?.(p)
      void qc.resetQueries()
    },
  })
}

/** Opens "Who's playing?", or straight to one player's PIN when `id` is a
 *  player with one (the top bar's switcher, Settings). */
export function askWhoIsPlaying(id?: number) {
  window.dispatchEvent(new CustomEvent('upnext:who', { detail: id ?? null }))
}

/** A name UpNext made up rather than one somebody chose: "Player 1", or
 *  "Player 2 2" when that was taken (`placeholder_name` in src/db/players.rs). */
export function placeholderName(name: string): boolean {
  return /^Player \d+( \d+)?$/.test(name)
}

/** Whose made-up name this browser has pointed out already. */
const NUDGED = 'upnext.player.nudged'
const nudgedNow = new Set<number>()

/** True the first time for player `id` in this browser (and, with storage
 *  turned off, the first time in this page), then false. */
export function nudgeOnce(id: number): boolean {
  const before = (read(local, NUDGED) ?? '').split(',').filter(Boolean)
  if (nudgedNow.has(id) || before.includes(String(id))) return false
  nudgedNow.add(id)
  write(local, NUDGED, [...before, String(id)].join(','))
  return true
}

/** "Sam's": a name as the owner of something. */
export function possessive(name: string): string {
  return /s$/i.test(name) ? `${name}’` : `${name}’s`
}

/** The first letter of a name, for a player's disc. */
export function initial(name: string): string {
  return Array.from(name.trim())[0]?.toUpperCase() ?? '?'
}

/** A PIN is four to eight digits. */
export function pinOk(pin: string): boolean {
  return /^\d{4,8}$/.test(pin)
}

/** The colours a player can take: the server's, in its order. */
export const COLOURS = ['#4f8cff', '#f2994a', '#27ae60', '#bb6bd9', '#eb5757', '#2bb3c0', '#e2b93b', '#9b8579']

/** What each "Hide this game" choice does, as its menu says it. */
export const HIDE_CHOICES: { scope: HideScope; label: string; hint: string; needsCopy: boolean }[] = [
  { scope: 'me', label: 'Just from me', hint: 'Out of your library, search and picks. Nobody else is affected.', needsCopy: false },
  {
    scope: 'others',
    label: 'From everyone else',
    hint: 'Your copy stays out of every other player’s UpNext, unless they own it themselves.',
    needsCopy: true,
  },
  { scope: 'everyone', label: 'From everyone', hint: 'Both: out of your views and everyone else’s.', needsCopy: true },
]

/** "Hidden from everyone else", for a scope. */
export function hiddenText(scope: HideScope): string {
  return scope === 'me' ? 'Hidden from you' : scope === 'others' ? 'Hidden from everyone else' : 'Hidden from everyone'
}

function listed(names: string[]): string {
  return names.length === 1 ? names[0] : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}

/** Who still sees a game hidden from the others, and why: "Sam has their
 *  own copy, so they'll still see it. Kit still sees it through someone
 *  else's shared account." Null when nobody does, and always for a game
 *  hidden only from you, which nobody else's view loses. */
export function stillSeenText(scope: HideScope, answer: Pick<HideAnswer, 'still_seen_by' | 'through_shared'>): string | null {
  if (scope === 'me') return null
  const shared = answer.through_shared
  const own = answer.still_seen_by.filter((n) => !shared.includes(n))
  const lines = []
  if (own.length === 1) lines.push(`${own[0]} has their own copy, so they’ll still see it.`)
  if (own.length > 1) lines.push(`${listed(own)} have their own copies, so they’ll still see it.`)
  if (shared.length) lines.push(`${listed(shared)} still ${shared.length === 1 ? 'sees' : 'see'} it through someone else’s shared account.`)
  return lines.length ? lines.join(' ') : null
}

/** Hidden by me asks for the PIN: the server answers 403 until it's entered. */
export function hiddenLocked(q: { error: unknown }): boolean {
  return q.error instanceof ApiError && q.error.status === 403
}

/** The line every place that hides games says once. */
export const HIDDEN_ONLY_HERE = 'Hidden in UpNext only. Anyone with this computer’s files can still see it.'

/** A setting's tag, with several players, when it is everyone's though it
 *  sits in a "You" section: changing it changes it for everyone. */
export function everyones(multi: boolean, tag?: string): string | undefined {
  return multi ? [tag, 'everyone’s'].filter(Boolean).join(' · ') : tag
}

/** The line Settings › Players, the picker and the profile say about privacy. */
export const NOT_PRIVATE =
  'Players aren’t private from this computer: anyone here can switch to a player without a PIN, and anyone with its files can read every player’s things.'
