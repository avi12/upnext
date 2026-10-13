import { useSyncExternalStore } from 'react'
import type { PriceInfo } from '../types'
import { loadJson, saveJson } from './format'

/** "Show costs in US dollars", remembered in this browser. Off by default: tokens are the same everywhere, currencies aren't. */
const KEY = 'upnext.showUsd'
const listeners = new Set<() => void>()
let showUsd = loadJson<boolean>(KEY, false)

function subscribe(listener: () => void) {
  listeners.add(listener)
  // Another tab changed it.
  const onStorage = (e: StorageEvent) => {
    if (e.key !== KEY) return
    showUsd = loadJson<boolean>(KEY, false)
    listener()
  }
  window.addEventListener('storage', onStorage)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

export function useShowUsd(): boolean {
  return useSyncExternalStore(subscribe, () => showUsd)
}

export function setShowUsd(on: boolean): void {
  showUsd = on
  saveJson(KEY, on)
  listeners.forEach((l) => l())
}

/** `n` with exactly `digits` decimals, in the reader's own way of writing numbers. */
function decimals(n: number, digits: number): string {
  return n.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits })
}

/** "US$0.09", "US$0.004", "US$12.40": always marked as US dollars, since UpNext doesn't convert currencies.
 *  Every amount is written the reader's way, so a comma-decimal locale never sees "US$0.004" beside "US$0,09";
 *  an amount that rounds to a whole cent is written in cents. */
export function usdText(n: number): string {
  if (n <= 0) return 'US$0'
  if (n < 0.001) return `under US$${decimals(0.001, 3)}`
  if (n < 0.0095) return `US$${decimals(n, 3)}`
  return `US$${decimals(n, 2)}`
}

/** What some calls cost: charged ("US$0.09"), and what wasn't charged at API prices ("US$0.09 at API prices"). Null when no price was known. */
export function costText(c: { cost_usd: number | null; api_usd: number | null }): string | null {
  const parts: string[] = []
  if (c.cost_usd != null) parts.push(usdText(c.cost_usd))
  const rest = c.api_usd != null ? c.api_usd - (c.cost_usd ?? 0) : 0
  if (c.api_usd != null && (rest > 1e-9 || c.cost_usd == null)) parts.push(`${usdText(Math.max(0, rest))} at API prices`)
  return parts.length ? parts.join(', ') : null
}

/** "Sep 30, 2026" */
function dayText(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

/** Where the prices come from and when they were read: "Anthropic's list prices, read Sep 30, 2026". */
export function pricesSource(p: PriceInfo): string {
  if (p.fetched_at) return `${p.label}'s list prices, read ${dayText(p.fetched_at)}`
  if (p.built_in_checked) return `${p.label}'s list prices as built into UpNext, checked ${p.built_in_checked}`
  return `${p.label}'s list prices`
}

/** The hover text on a dollar figure. */
export function pricesTitle(p: PriceInfo | null | undefined): string {
  return p ? `At ${pricesSource(p)}. They may be out of date; ${p.label}'s pricing page has the current ones.` : 'At list prices, which may be out of date.'
}
