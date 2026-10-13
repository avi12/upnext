/** The code a pairing QR code carries in the address's fragment
 *  (`/#pair=482913`). A fragment never reaches the server, and the
 *  UI takes it out of the address as it loads, so the code stays in neither
 *  the history nor a bookmark. */
export function codeInHash(hash: string): string | null {
  const m = /^#pair=(\d{6})$/.exec(hash)
  return m ? m[1] : null
}

const found = typeof window === 'undefined' ? null : codeInHash(window.location.hash)
if (found) {
  window.history.replaceState(window.history.state, '', window.location.pathname + window.location.search)
}

/** The code the address carried when the UI loaded, if it carried one. */
export function pairCodeFromAddress(): string | null {
  return found
}
