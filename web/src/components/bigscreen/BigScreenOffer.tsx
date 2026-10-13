import { useEffect } from 'react'
import { OFFERED_KEY, setBigScreen } from '../../lib/bigscreen'
import { whenFocused } from '../../lib/gamepad/focus'
import { useToast } from '../useToast'

/** In the normal layout, the first gamepad press offers big screen, once per
 *  browser. A browser only tells a page about a pad once a button on it is
 *  pressed, so "connected" here means someone is holding one. The first-run
 *  wizard switches by itself instead (`SetupPage`). */
export function BigScreenOffer() {
  const toast = useToast()
  useEffect(() => {
    const onPad = () => {
      if (window.location.pathname === '/setup') return
      try {
        if (localStorage.getItem(OFFERED_KEY)) return
        localStorage.setItem(OFFERED_KEY, '1')
      } catch {
        return
      }
      toast.info('Switch to big screen?', 'Big screen is made for a controller: large type, and the pad moves around.', {
        label: 'Switch to big screen',
        onClick: () => setBigScreen(true),
      })
    }
    return whenFocused(() => {
      window.addEventListener('gamepadconnected', onPad)
      return () => window.removeEventListener('gamepadconnected', onPad)
    })
  }, [toast])
  return null
}
