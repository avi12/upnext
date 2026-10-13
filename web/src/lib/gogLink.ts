// GOG's sign-in through the "Send to UpNext" bookmark, for
// a browser that isn't on the computer UpNext runs on: Docker, another device.
// GOG only sends its code to its own page, so a bookmarklet clicked there
// brings the code to this UpNext's /gog/callback page, in the address's
// fragment (which never reaches a server). That page sends it with the
// one-time state this browser got from POST /gog/browser.
//
// The bookmark also carries this install's key (security review L10): the
// callback page finds the state in this browser's storage, so without the
// key another site could send the browser there with a GOG code of its own
// while a sign-in waits. That site can't know the key.

/** Where this browser keeps the sign-in it started. */
const KEY = 'upnext.gog.signin'

export interface GogSignIn {
  state: string
  /** The page to go back to: the wizard or Settings. */
  back: string
  expires_at: number
}

/** The bookmarklet for an UpNext at `origin` with this install's `key`: on
 *  GOG's success page it sends the tab to `origin`/gog/callback with the key
 *  and the code; anywhere else it says where to use it. */
export function bookmarklet(origin: string, key: string) {
  const to = `${origin}/gog/callback#key=${encodeURIComponent(key)}&code=`
  const code = `(()=>{var c=new URLSearchParams(location.search).get('code');if(!c){alert(${JSON.stringify(
    'Send to UpNext: click this on GOG’s page after you sign in. Its address has code= in it.',
  )});return}location.href=${JSON.stringify(to)}+encodeURIComponent(c)})()`
  return `javascript:${encodeURIComponent(code)}`
}

function fromHash(hash: string, name: string) {
  const v = new URLSearchParams(hash.replace(/^#/, '')).get(name)
  return v && v.trim() ? v.trim() : null
}

/** The code in /gog/callback's fragment, or null. */
export function codeFromHash(hash: string) {
  return fromHash(hash, 'code')
}

/** The bookmark's key in /gog/callback's fragment, or null (a bookmark from
 *  before it had one). */
export function keyFromHash(hash: string) {
  return fromHash(hash, 'key')
}

export function saveSignIn(s: GogSignIn) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s))
  } catch {
    // No storage (a private window that refuses it): the callback page says to paste instead.
  }
}

/** The sign-in this browser started, taken out so it's used once; null when
 *  there's none or it has expired. `now` is in seconds. */
export function takeSignIn(now = Date.now() / 1000): GogSignIn | null {
  let raw: string | null = null
  try {
    raw = localStorage.getItem(KEY)
    localStorage.removeItem(KEY)
  } catch {
    return null
  }
  if (!raw) return null
  try {
    const s = JSON.parse(raw) as Partial<GogSignIn>
    if (typeof s.state !== 'string' || typeof s.expires_at !== 'number' || s.expires_at <= now) return null
    // Only back to a page of this UpNext's own.
    const back = typeof s.back === 'string' && s.back.startsWith('/') && !s.back.startsWith('//') ? s.back : '/settings'
    return { state: s.state, back, expires_at: s.expires_at }
  } catch {
    return null
  }
}
