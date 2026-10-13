import { describe, expect, it } from 'vitest'
import type { Backup } from '../../types'
import { restoreProblem } from './backupText'

const at = (schema: number | null) => ({ name: 'upnext-manual-20261001-101500.db', kind: 'manual', made_at: 0, bytes: 1, schema }) as Backup

describe('Settings, Backups: which backups Restore takes', () => {
  it('takes one from the first release on, up to this build', () => {
    expect(restoreProblem(at(29), 33, 29)).toBeNull()
    expect(restoreProblem(at(33), 33, 29)).toBeNull()
  })

  it('greys out one from before the first release, as the server refuses it', () => {
    expect(restoreProblem(at(28), 33, 29)).toMatch(/^It's from a development build of UpNext made before its first release \(schema v28\)/)
  })

  it('leaves an unnumbered database to the server, which says what it holds', () => {
    expect(restoreProblem(at(0), 33, 29)).toBeNull()
  })

  it('greys out a newer one and a file that is no database', () => {
    expect(restoreProblem(at(34), 33, 29)).toMatch(/newer UpNext/)
    expect(restoreProblem(at(null), 33, 29)).toBe("It isn't a database")
  })
})
