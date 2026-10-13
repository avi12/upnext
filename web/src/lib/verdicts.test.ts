import { describe, expect, it } from 'vitest'
import { verdictsContract } from '../test/verdictsContract'
import type { MyVerdict } from '../types'
import { fieldShows, shownAnswers, stateText, textRefusal } from './verdicts'

const contract = verdictsContract
const field = (name: string) => contract.fields.find((f) => f.name === name)!

describe('backlog verdict form rules', () => {
  it('shows factors only with limited time, the note with a stopping point, and what stopped you only when you stopped', () => {
    expect(fieldShows(field('factors'), {})).toBe(false)
    expect(fieldShows(field('factors'), { limited_time: 'yes' })).toBe(true)
    expect(fieldShows(field('stop_note'), { stop_point: 'anytime' })).toBe(true)
    expect(fieldShows(field('stopped_for'), { outcome: 'finished' })).toBe(false)
    expect(fieldShows(field('stopped_for'), { outcome: 'shelved' })).toBe(true)
  })

  it("never sends an answer its form didn't show", () => {
    const sent = shownAnswers(contract.fields, { outcome: 'finished', worth: 'yes', factors: ['recap'], stop_note: 'after the boss', stopped_for: ['grind'] })
    expect(sent.factors).toEqual([])
    expect(sent.stop_note).toBeNull()
    expect(sent.stopped_for).toEqual([])
    expect(sent.outcome).toBe('finished')
  })

  it("refuses what the service refuses, after the characters it ignores", () => {
    expect(textRefusal(contract, 'Great pacing, short chapters.', 500)).toBeNull()
    expect(textRefusal(contract, 'see www.example.com', 500)).toBe('It has a link or a web address; take it out to send.')
    expect(textRefusal(contract, 'mail me at someone@example.org', 500)).toMatch(/email address/)
    expect(textRefusal(contract, 'steam​community', 500)).toMatch(/Steam profile/)
    expect(textRefusal(contract, 'x'.repeat(81), 80)).toBe("It's 81 characters; at most 80.")
    expect(textRefusal(contract, '   ', 80)).toBeNull()
  })

  it('says where a verdict stands', () => {
    const v = { state: 'draft', sent_at: null } as MyVerdict
    expect(stateText(v)).toBe('Draft, not sent')
    expect(stateText({ ...v, remote_id: '01JA0000000000000000000000' })).toBe('Changes not sent; your earlier verdict is still up')
    expect(stateText({ ...v, state: 'queued' })).toBe('Waiting to be sent')
    expect(stateText({ ...v, state: 'queued', sent_at: 1_790_000_000 })).toBe('Your changes are waiting to be sent')
    expect(stateText({ ...v, state: 'sent', sent_at: 1_790_000_000 })).toMatch(/^Sent /)
    expect(stateText({ ...v, state: 'withdrawn' })).toBe('Taken down')
  })
})
