import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useSearchParams } from 'react-router'
import { useToast } from '../components/useToast'
import { keys } from './queries'

/** What a page says when the browser comes back to it from a sign-in. */
export interface SignInWords {
  /** The detail under "Signed in with Steam". */
  steam: string
  /** The detail under "GOG connected". */
  gog: string
  /** Whether "GOG login didn't work" is said too. */
  gogErrors?: boolean
}

/** Back from Steam's sign-in (`?steam=ok`, or `?steam=error&reason=…`), or
 *  from GOG's through the bookmark (`?gog=…`): one toast, the parameters
 *  taken out of the address, and the settings read again. */
export function useSignInReturn(words: SignInWords) {
  const qc = useQueryClient()
  const toast = useToast()
  const [params, setParams] = useSearchParams()
  // React's development build runs a page's first effects twice, with the
  // address from before it was cleaned up; the toast is said once.
  const done = useRef(false)
  useEffect(() => {
    if (done.current) return
    done.current = true
    const steam = params.get('steam')
    const gog = params.get('gog')
    if (!steam && !gog) return
    if (steam === 'ok') toast.success('Signed in with Steam', words.steam)
    else if (steam) toast.error("Steam sign-in didn't work", params.get('reason') ?? undefined)
    if (gog === 'ok') toast.success('GOG connected', words.gog)
    else if (gog && words.gogErrors) toast.error("GOG login didn't work", params.get('reason') ?? undefined)
    const next = new URLSearchParams(params)
    next.delete('steam')
    next.delete('gog')
    next.delete('reason')
    setParams(next, { replace: true })
    void qc.invalidateQueries({ queryKey: keys.settings })
    void qc.invalidateQueries({ queryKey: keys.gog })
    // Once, as the page opens: the address is cleaned up straight after.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
}
