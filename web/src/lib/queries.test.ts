import { describe, expect, it } from 'vitest'
import { keys } from './queries'

/** Every source file but the tests, the generated client and the keys' own. */
const sources = import.meta.glob(['../**/*.{ts,tsx}', '!../**/*.test.{ts,tsx}', '!../generated/**', '!./queries.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

/** A line that reads, writes or invalidates a query. */
const QUERY_LINE = /queryKey|QueryData|(?:invalidate|refetch|remove|cancel)Queries/

/** The shared keys a line writes out in an array, in either kind of quotes:
 *  `queryKey: ['settings']`, `setQueryData(["gog"], …)`, or a loop over
 *  `['profile', 'settings']` that invalidates each. */
function writtenOut(line: string): string[] {
  if (!QUERY_LINE.test(line)) return []
  const names = Object.values(keys).flatMap((k) => (k.length === 1 ? [k[0]] : []))
  return [...line.matchAll(/\[([^\]]*)\]/g)].flatMap((m) =>
    [...m[1].matchAll(/(['"])([\w-]+)\1/g)].map((q) => q[2]).filter((n) => (names as string[]).includes(n)),
  )
}

describe('the shared query keys (R-WEB-4)', () => {
  it('are written out only in lib/queries.ts', () => {
    expect(Object.keys(sources).length).toBeGreaterThan(100)
    const found = Object.entries(sources).flatMap(([file, text]) =>
      text.split('\n').flatMap((line, i) => writtenOut(line).map((n) => `${file}:${i + 1}: ${n}`)),
    )
    expect(found).toEqual([])
  })

  it('finds them in either kind of quotes, and in a loop', () => {
    expect(writtenOut(`useQuery({ queryKey: ['settings'], queryFn })`)).toEqual(['settings'])
    expect(writtenOut(`qc.invalidateQueries({ queryKey: ["players"] })`)).toEqual(['players'])
    expect(writtenOut(`qc.setQueryData<X>(['setup-state'], s)`)).toEqual(['setup-state'])
    expect(writtenOut(`for (const k of ['profile', "config", 'setup']) qc.invalidateQueries({ queryKey: [k] })`)).toEqual(['profile', 'config'])
    expect(writtenOut(`qc.invalidateQueries({ queryKey: ['game', id] })`)).toEqual([])
    expect(writtenOut(`const tabs = ['settings', 'players']`)).toEqual([])
  })
})
