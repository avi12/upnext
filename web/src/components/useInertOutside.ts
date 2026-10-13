import { useEffect, type RefObject } from 'react'

/** While a modal is shown, the rest of the page is inert: out of the Tab
 *  order and out of a screen reader's reach. `aria-modal` alone doesn't keep
 *  every screen reader in: Orca's arrow keys left the Allow dialog for the
 *  page behind it in Brave. Every element beside `root` and beside each of
 *  its ancestors is made inert, and only those made so here are freed when
 *  the modal goes. */
export function useInertOutside(root: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const made: Element[] = []
    for (let at = root.current; at?.parentElement && at !== document.body; at = at.parentElement) {
      for (const beside of at.parentElement.children) {
        if (beside === at || beside.hasAttribute('inert') || beside.tagName === 'SCRIPT') continue
        beside.setAttribute('inert', '')
        made.push(beside)
      }
    }
    return () => made.forEach((el) => el.removeAttribute('inert'))
  }, [root])
}
