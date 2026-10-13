import type { VerdictContract } from '../types'

// The verdicts' contract as docs/verdicts-contract.json holds it, for tests.
// It's outside web/, which the container image's build copies alone, so it
// is read through Vite's glob, which the type check doesn't follow.
const found = Object.values(import.meta.glob('../../../docs/verdicts-contract.json', { eager: true, import: 'default' }))
if (found.length !== 1) throw new Error('docs/verdicts-contract.json not found')

export const verdictsContract = found[0] as VerdictContract
