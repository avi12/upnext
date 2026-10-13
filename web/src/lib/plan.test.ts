import { describe, expect, it } from 'vitest'
import type { PlanSummary } from '../types'
import { pctText, pickShareText, planTitle, windowName, windowNoun } from './plan'

describe('plan', () => {
  it('names the plan', () => {
    expect(planTitle({ backend: 'claude-cli', plan: 'max' })).toBe('Your Claude plan (Max)')
    expect(planTitle({ backend: 'codex-cli', plan: 'prolite' })).toBe('Your ChatGPT plan (Pro Lite)')
    expect(planTitle({ backend: 'codex-cli', plan: null })).toBe('Your ChatGPT plan')
  })

  it('names windows', () => {
    expect(windowName('session', 'Current session')).toBe('This session')
    expect(windowName('week:Fable', 'x')).toBe('This week, Fable')
    expect(windowName('session', '5-hour limit', 'codex-cli')).toBe('5-hour limit')
    expect(windowNoun('session', 'codex-cli')).toBe('the 5-hour window')
    expect(windowNoun('week:Fable')).toBe("Fable's week")
  })

  it('writes shares', () => {
    expect(pctText(41.4)).toBe('about 41%')
    expect(pctText(0.3)).toBe('about 0.3%')
    expect(pctText(0.05)).toBe('about 0.05%')
    expect(pctText(0.001)).toBe('under 0.01%')
  })

  it("puts a pick's share of each window in words, without a model's own week", () => {
    const plan = {
      backend: 'claude-cli',
      measured: {
        measure: 'api_usd',
        spans: 3,
        windows: [
          { id: 'session', label: '', per_point: 0.3, spans: 3, from_session: null },
          { id: 'week', label: '', per_point: 1.5, spans: 3, from_session: null },
          { id: 'week:Fable', label: '', per_point: 1, spans: 3, from_session: null },
        ],
        each: [],
        one_time: null,
      },
    } as unknown as PlanSummary
    expect(pickShareText(plan, { api_usd: 0.09, usage: null })).toBe('about 0.3% of the session and about 0.06% of the week')
    expect(pickShareText(plan, { api_usd: null, usage: null })).toBeNull()
  })
})
