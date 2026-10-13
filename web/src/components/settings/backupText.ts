import type { Backup } from '../../types'

/** Why a backup can't be restored, or null when it can. */
export function restoreProblem(b: Backup, schema: number, oldest: number): string | null {
  if (b.schema == null) return "It isn't a database"
  if (b.schema > schema) return `It's from a newer UpNext (schema v${b.schema}); this one reads up to v${schema}`
  // 0 is a database UpNext never wrote to; the server says what it holds.
  if (b.schema > 0 && b.schema < oldest)
    return `It's from a development build of UpNext made before its first release (schema v${b.schema}); this one restores v${oldest} and later`
  return null
}
