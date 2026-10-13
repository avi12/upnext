import type { Settings } from '../../types'

/** Who the SteamID is, and how UpNext knows it: a sign-in or a SteamID typed
 *  in the app, the Steam client's files, or the environment. */
export function steamIdText(s: Pick<Settings, 'steam_id' | 'steam_id_source' | 'steam_persona'>): string {
  const who = s.steam_persona ?? s.steam_id ?? ''
  switch (s.steam_id_source) {
    case 'steam client':
      return `Found on this computer: ${who}`
    case 'env':
      return `${who}, from .env`
    default:
      return `Signed in as ${who}`
  }
}
