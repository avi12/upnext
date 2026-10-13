// A page open across an upgrade (code health B-WEB-1): its pages load the
// first time they're opened, by the names of the build that served it, and
// the new build has other names, so the server answers 404. Loading the page
// again brings the new build. Once: if the script is still missing after
// that, the error boundary says so instead of reloading in a loop.

/** When this page last reloaded itself, in this tab. */
const KEY = 'upnext.reloaded-for-update'
/** A second failure within this long after a reload is shown, not reloaded. */
const AGAIN_AFTER_MS = 60_000

let underway = false

/** The page's own reload, apart so tests can watch it. */
export const page = {
  reload: () => window.location.reload(),
}

/** Whether `error` is one of the page's own scripts failing to load, as
 *  Chromium, Firefox and Safari word it, or Vite's own preload. */
export function scriptMissing(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error)
  return /dynamically imported module|Importing a module script failed|Unable to preload CSS|Failed to load module script/i.test(text)
}

/** Load the page again, unless it did so for the same reason less than a
 *  minute ago or can't remember whether it did. True when it reloads. */
export function reloadOnce(now = Date.now()): boolean {
  if (underway) return true
  try {
    const last = Number(window.sessionStorage.getItem(KEY)) || 0
    if (now - last >= 0 && now - last < AGAIN_AFTER_MS) return false
    window.sessionStorage.setItem(KEY, String(now))
  } catch {
    // Without storage a reload couldn't tell it had happened, and could loop.
    return false
  }
  underway = true
  page.reload()
  return true
}

/** Whether the page is loading itself again. */
export function reloading(): boolean {
  return underway
}

/** Vite's event for a page's script or style that didn't load: reload once,
 *  and keep the error from the page, which is going away. */
export function reloadOnStaleScripts() {
  window.addEventListener('vite:preloadError', (event) => {
    if (reloadOnce()) event.preventDefault()
  })
}

/** For tests: forget a reload. */
export function resetReload() {
  underway = false
}
