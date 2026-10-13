import type { SteamShortcut } from '../types'

/** Add to Steam gives the shortcut Steam's Gamepad controller layout; this is the way to pick it by hand when Steam has another. */
export const PAD_LAYOUT_HELP =
  'If the controller moves the mouse there, pick Steam’s Gamepad layout for it: on UpNext’s page in Steam, the controller icon, then the layout, then Templates, Gamepad.'

/** What "Add to Steam" says, and whether its button does anything now. */
export function steamShortcutState(sc: SteamShortcut): { text: string; canAdd: boolean; added: boolean } {
  if (sc.unavailable) return { text: sc.unavailable, canAdd: false, added: false }
  const accounts = sc.accounts
  // One file UpNext can't read stops it adding to any account.
  const unreadable = accounts.find((a) => a.error)
  if (unreadable) {
    return { text: `Steam’s shortcuts for account ${unreadable.account_id} can’t be read: ${unreadable.error}`, canAdd: false, added: false }
  }
  const added = accounts.length > 0 && accounts.every((a) => a.added)
  // A shortcut added before Add to Steam wrote controller layouts gets one by adding again.
  // When writing the layout failed, pressing again would fail the same way: say why, and how to pick it in Steam.
  const unwritten = added ? accounts.find((a) => !a.layout && a.layout_error) : undefined
  if (unwritten) {
    return {
      text: `UpNext is in Steam’s library, but its controller layout couldn’t be written for account ${unwritten.account_id}: ${unwritten.layout_error}. ${PAD_LAYOUT_HELP}`,
      canAdd: false,
      added: true,
    }
  }
  if (added && accounts.some((a) => !a.layout)) {
    if (sc.steam_running) return { text: 'Close Steam first, then add it again to give it Steam’s Gamepad controller layout.', canAdd: true, added: false }
    return { text: 'UpNext is in Steam’s library without a controller layout: add it again to give it Steam’s Gamepad layout.', canAdd: true, added: false }
  }
  if (added) {
    return {
      text:
        (accounts.length === 1
          ? 'UpNext is in Steam’s library, under Non-Steam, and opens in big screen.'
          : `UpNext is in the library of all ${accounts.length} Steam accounts here, under Non-Steam.`) +
        ` ${PAD_LAYOUT_HELP}`,
      canAdd: false,
      added: true,
    }
  }
  if (sc.steam_running) return { text: 'Close Steam first: it rewrites its shortcuts as it exits, which would undo this.', canAdd: true, added: false }
  return {
    text:
      accounts.length === 1
        ? 'Puts UpNext in Steam’s library, under Non-Steam, opening in big screen: Big Picture and Gaming mode can start it.'
        : `Puts UpNext in the library of each of the ${accounts.length} Steam accounts here, opening in big screen.`,
    canAdd: true,
    added: false,
  }
}

/** The launchers on a handheld that can't be told about UpNext by a program,
 *  with how to add it by hand. Windows only: on Linux, Steam is the one. */
export const LAUNCHER_STEPS: { name: string; steps: string }[] = [
  {
    name: 'The Xbox app',
    steps: 'In its library, choose + to add a game or app, then pick UpNext (big screen) from the list, or browse to the program below and give it the arguments.',
  },
  {
    name: 'Armoury Crate SE (ROG)',
    steps: 'In its game library, add a game, browse to the program below, and give it the arguments if it asks for them.',
  },
  {
    name: 'Legion Space (Lenovo)',
    steps: 'In its library, add a game, browse to the program below, and give it the arguments if it asks for them.',
  },
]
