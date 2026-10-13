import { inApp } from '../frame'

// WebKitGTK, the engine of the app window on Linux, starts reading the
// computer's pads the first time a page asks for them (a `gamepadconnected`
// listener, or `navigator.getGamepads()`), but only when the window has the
// focus right then; it doesn't try again when the window gets the focus
// later. Asked while the window is in the background (opened behind another
// window, or under a locked screen), it never starts, and a pad plugged in
// after that makes its first button press spin the window at full CPU until
// a page asks again with the window focused (WebKitGTK 2.52).
// So in the app window the pads are left alone until it has had the focus.

/** Whether the app window has had the focus since this page loaded. */
let hadFocus = false

/** Run `start`, which may return what undoes it, once the page may touch
 *  the pads: at once in a browser, or in an app window that has had the
 *  focus; else when the window first gets it. Returns what stops it, or
 *  the wait for the focus. */
export function whenFocused(start: () => void | (() => void)): () => void {
  if (hadFocus || !inApp() || document.hasFocus()) {
    hadFocus = true
    const stop = start()
    return () => stop?.()
  }
  let stop: void | (() => void)
  let cancelled = false
  const focused = () => {
    window.removeEventListener('focus', focused)
    // Sent by the window itself when it gets the focus (`FOCUS_SCRIPT` in
    // src/desktop.rs), in case the page's own event doesn't come.
    window.removeEventListener('upnext:focus', focused)
    hadFocus = true
    if (!cancelled) stop = start()
  }
  window.addEventListener('focus', focused)
  window.addEventListener('upnext:focus', focused)
  return () => {
    cancelled = true
    window.removeEventListener('focus', focused)
    window.removeEventListener('upnext:focus', focused)
    stop?.()
  }
}
