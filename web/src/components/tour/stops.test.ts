import { describe, expect, it } from 'vitest'
import { TOUR_STOPS, tourAnchors } from './stops'

// The UI's source, to find the anchors in (a renamed component
// mustn't drop a stop silently).
const sources = import.meta.glob(['../../**/*.tsx', '!../../**/*.test.tsx'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>

describe('the tour', () => {
  it('finds every part it points at in the UI', () => {
    const ui = Object.values(sources).join('\n')
    expect(Object.keys(sources).length).toBeGreaterThan(50)
    const missing = tourAnchors().filter((id) => !ui.includes(`data-tour="${id}"`))
    expect(missing).toEqual([])
  })

  it('has about ten stops, each with its own id and words', () => {
    expect(TOUR_STOPS.length).toBe(10)
    expect(new Set(TOUR_STOPS.map((s) => s.id)).size).toBe(TOUR_STOPS.length)
    const ctx = { lineup: 'empty', ai: true, example: false, sheetGame: true }
    for (const s of TOUR_STOPS) {
      expect(s.path.startsWith('/'), s.id).toBe(true)
      expect(s.title.length, s.id).toBeGreaterThan(3)
      expect(s.body(ctx).length, s.id).toBeGreaterThan(20)
    }
  })

  it('fills the lineup stop with an example rather than offering to spend an AI call', () => {
    const ask = TOUR_STOPS.find((s) => s.id === 'ask')!
    expect(ask.body({ lineup: null, ai: true, example: false, sheetGame: false })).not.toMatch(/Or try/)
    const lineup = TOUR_STOPS.find((s) => s.id === 'lineup')!
    expect(lineup.example).toBe(true)
    const filled = { lineup: 'filled', ai: false, example: true, sheetGame: false }
    expect(lineup.body(filled)).toMatch(/^Three from your library as an example, ranked by UpNext alone\. With an AI set up/)
    expect(lineup.body({ ...filled, example: false })).toMatch(/^Each pick says why it fits/)
    expect(lineup.body({ ...filled, lineup: 'empty' })).not.toMatch(/asks the AI once/)
  })
})
