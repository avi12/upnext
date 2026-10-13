import { describe, expect, it } from 'vitest'
import type { Job, LaneProgress } from '../types'
import { isPartial, labelMid, lanesText, lowerFirst, progressText, resultText, tookText, troubleText } from './jobs'

describe('jobs', () => {
  it('writes how long something took', () => {
    expect(tookText(-3)).toBe('0 s')
    expect(tookText(42)).toBe('42 s')
    expect(tookText(185)).toBe('3 min')
    expect(tookText(3600)).toBe('1 h')
    expect(tookText(5880)).toBe('1 h 38 min')
    expect(tookText(4 * 86400)).toBe('4 days')
  })

  it('lowers labels mid-sentence, but not names', () => {
    expect(lowerFirst('Looking up RAWG')).toBe('looking up RAWG')
    expect(lowerFirst('RAWG')).toBe('RAWG')
    expect(labelMid('sync', 'Sync libraries')).toBe('sync libraries')
    expect(labelMid('digests', 'People say')).toBe('People say')
  })

  it('says what the lanes have done while no game is counted yet', () => {
    const lane = (name: string, done: number, total: number, state: LaneProgress['state'] = 'running') =>
      ({ name, done, total, failed: 0, state, current: null, note: null, retry_at: null }) as LaneProgress
    const job = (done: number, lanes: LaneProgress[]) => ({ done, total: 1600, lanes, source: 'ui', eta_secs: null, message: '', ai_usage: null }) as unknown as Job
    expect(lanesText(job(0, [lane('Wikidata', 120, 261), lane('Series covers', 40, 300)]))).toBe('Wikidata 120 of 261')
    expect(lanesText(job(0, [lane('Wikidata', 261, 261, 'done'), lane('Series covers', 40, 300), lane('Steam store', 0, 1600)]))).toBe(
      '1 of 3 sources done',
    )
    expect(progressText(job(0, [lane('Wikidata', 261, 261, 'done'), lane('Steam store', 0, 1600)]))).toBe('1 of 2 sources done')
    // Nothing to say before any lane has moved, nor once games are counted.
    expect(lanesText(job(0, [lane('Wikidata', 0, 261)]))).toBeNull()
    expect(lanesText(job(5, [lane('Wikidata', 261, 261, 'done')]))).toBeNull()
    expect(progressText(job(5, [lane('Wikidata', 261, 261, 'done')]))).toBe('5 of 1,600')
  })

  it('says a sync with a failed account is partial, not done', () => {
    const job = (kind: string, status: Job['status'], failed: number, summary: string | null) => ({ kind, status, failed, summary }) as unknown as Job
    const partial = job('sync', 'done', 1, 'Partial: 2,000 games, 3 new; Steam (Sam) failed')
    expect(isPartial(partial)).toBe(true)
    expect(resultText(partial)).toBe('Partial: 2,000 games, 3 new; Steam (Sam) failed')
    expect(troubleText(partial, 'Sync')).toBe('Sync: partial, 1 failed')
    const whole = job('sync', 'done', 0, '2,000 games, 3 new')
    expect(isPartial(whole)).toBe(false)
    expect(resultText(whole)).toBe('Done: 2,000 games, 3 new')
    expect(resultText(job('enrich', 'done', 3, '1,600 titles'))).toBe('Done: 1,600 titles')
    expect(troubleText(job('enrich', 'done', 3, null), 'Game details')).toBe('Game details: 3 failed')
    expect(troubleText(job('enrich', 'cancelled', 3, null), 'Game details')).toBe('Game details: stopped, 3 failed')
    expect(troubleText(job('sync', 'failed', 0, null), 'Sync')).toBe('Sync failed')
    expect(resultText(job('sync', 'cancelled', 0, null))).toBe('Cancelled')
  })
})
