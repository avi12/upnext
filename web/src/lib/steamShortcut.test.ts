import { describe, expect, it } from 'vitest'
import type { SteamShortcut } from '../types'
import { steamShortcutState } from './steamShortcut'

const base: SteamShortcut = {
  target: { program: '/usr/bin/upnext', arguments: 'app --big-screen' },
  accounts: [{ account_id: '12345678', added: false, layout: false }],
  steam_running: false,
  unavailable: null,
}

describe('Add to Steam', () => {
  it('offers to add, says Steam has to close first, and says when it is there', () => {
    expect(steamShortcutState(base)).toMatchObject({ canAdd: true, added: false })
    expect(steamShortcutState({ ...base, steam_running: true }).text).toMatch(/^Close Steam first/)
    const added = steamShortcutState({ ...base, accounts: [{ account_id: '12345678', added: true, layout: true }] })
    expect(added).toMatchObject({ canAdd: false, added: true })
    // With the way to pick the Gamepad layout by hand.
    expect(added.text).toContain('pick Steam’s Gamepad layout')
    // Every account, or it isn't done.
    const some = steamShortcutState({ ...base, accounts: [{ account_id: '1', added: true, layout: true }, { account_id: '2', added: false, layout: false }] })
    expect(some).toMatchObject({ canAdd: true, added: false })
    expect(some.text).toContain('2 Steam accounts')
  })

  it('says why and stops offering the button when the layout could not be written', () => {
    const failed = { ...base, accounts: [{ account_id: '12345678', added: true, layout: false, layout_error: 'writing it: permission denied' }] }
    const state = steamShortcutState(failed)
    expect(state).toMatchObject({ canAdd: false, added: true })
    expect(state.text).toContain('permission denied')
    expect(state.text).toContain('pick Steam’s Gamepad layout')
  })

  it('offers to add again when the shortcut has no controller layout yet', () => {
    const without = { ...base, accounts: [{ account_id: '12345678', added: true, layout: false }] }
    const state = steamShortcutState(without)
    expect(state).toMatchObject({ canAdd: true, added: false })
    expect(state.text).toContain('without a controller layout')
    expect(steamShortcutState({ ...without, steam_running: true }).text).toMatch(/^Close Steam first, then add it again/)
  })

  it('adds to no account while one account’s file cannot be read', () => {
    const state = steamShortcutState({
      ...base,
      accounts: [
        { account_id: '1', added: false, layout: false, error: null },
        { account_id: '2', added: false, layout: false, error: 'not Steam’s shortcuts file' },
      ],
    })
    expect(state).toMatchObject({ canAdd: false, added: false })
    expect(state.text).toBe('Steam’s shortcuts for account 2 can’t be read: not Steam’s shortcuts file')
  })

  it('says why it cannot', () => {
    const state = steamShortcutState({ ...base, target: null, accounts: [], unavailable: 'UpNext runs in a container here' })
    expect(state).toEqual({ text: 'UpNext runs in a container here', canAdd: false, added: false })
  })
})
