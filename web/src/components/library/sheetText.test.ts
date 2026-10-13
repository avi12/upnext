import { describe, expect, it } from 'vitest'
import type { Copy, GameView } from '../../types'
import { editionsText, ownedText, storeVia } from './sheetText'

function title(...editions: Array<string | null>): Pick<GameView, 'copies' | 'editions'> {
  const named = [...new Set(editions.filter((e): e is string => !!e))].sort()
  return { copies: editions.map((edition) => ({ edition }) as Copy), editions: named }
}

describe('The sheet header’s editions', () => {
  it('names each edition when a remaster shares the title with the original', () => {
    expect(editionsText(title(null, 'Remastered'))).toBe('2 editions: Original, Remastered')
    expect(editionsText(title(null, 'Enhanced', 'Remastered', 'Remastered'))).toBe('3 editions: Original, Enhanced, Remastered')
  })

  it('names the one edition every copy is, and says nothing for plain copies', () => {
    expect(editionsText(title('Remastered', 'Remastered'))).toBe('Remastered')
    expect(editionsText(title(null, null))).toBeNull()
  })
})

describe('The sheet header’s stores', () => {
  const owned = (...owned_on: string[]) => ({ owned_on, source: owned_on[0] }) as Pick<GameView, 'owned_on' | 'source'>

  it('lists every store in one order, whichever Play launches', () => {
    expect(ownedText(owned('epic', 'steam'))).toBe('Owned on Steam and Epic')
    expect(ownedText(owned('gog', 'origin', 'steam'))).toBe('Owned on Steam, GOG and EA app')
    expect(ownedText(owned('steam'))).toBe('Owned on Steam')
  })
})

describe('Where a copy came from', () => {
  it('names the launcher only when it is not the store itself', () => {
    expect(storeVia({ source: 'epic', launcher: 'epic' })).toBe('Epic')
    expect(storeVia({ source: 'epic', launcher: 'heroic' })).toBe('Epic via Heroic')
    expect(storeVia({ source: 'origin', launcher: 'origin' })).toBe('EA app')
    expect(storeVia({ source: 'uplay', launcher: 'galaxy' })).toBe('Ubisoft Connect via GOG Galaxy')
  })

  it('says a game on a list was added by hand', () => {
    expect(storeVia({ source: 'origin', launcher: 'manual' })).toBe('EA app, added by hand')
  })
})
