// The app window using another computer's UpNext. The
// window opens on the computer chosen on the connect page, and tells that
// computer's pages which one they are (`window.upnextRemote`), so Settings
// can say so and offer this computer's library again.
import './frame'

declare global {
  interface Window {
    /** Set by the app window on another computer's pages (`remote_script` in src/desktop.rs). */
    upnextRemote?: { name?: string }
  }
}

/** The other computer this window shows, or null: this computer's own
 *  UpNext, or a browser. */
export function remoteName(): string | null {
  if (typeof window === 'undefined') return null
  const name = window.upnextRemote?.name
  return typeof name === 'string' && name ? name : null
}

/** Whether the page can ask the window to open elsewhere: it's in the app
 *  window, which listens for `connect:` messages. */
export function canSwitch(): boolean {
  return typeof window !== 'undefined' && window.upnextApp === true && typeof window.ipc?.postMessage === 'function'
}

/** Open the window on a saved computer, by its id; null for this
 *  computer's own UpNext again. The window remembers the choice. */
export function openComputer(id: string | null) {
  window.ipc?.postMessage(`connect:${id ?? 'here'}`)
}

/** The saved computer the window didn't open, from the connect page's
 *  address, and why: nothing answered at its address
 *  (`/connect?unreachable=<id>`), or another UpNext did
 *  (`/connect?changed=<id>`), which wasn't sent its token. */
export function missedComputer(search: string): { id: string; changed: boolean } | null {
  const params = new URLSearchParams(search)
  for (const [key, changed] of [
    ['changed', true],
    ['unreachable', false],
  ] as const) {
    const id = params.get(key)
    if (id && /^[A-Za-z0-9]{1,64}$/.test(id)) return { id, changed }
  }
  return null
}

/** "a phone", for "A phone called "Pixel 9" wants to use UpNext". */
export function deviceKindText(kind: string): string {
  switch (kind) {
    case 'phone':
      return 'A phone'
    case 'tablet':
      return 'A tablet'
    case 'tv':
      return 'A TV'
    default:
      return 'A computer'
  }
}
