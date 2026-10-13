import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import { codeFromHash, keyFromHash, takeSignIn, type GogSignIn } from '../lib/gogLink'
import { PlayMark } from '../components/Shell'
import { keys } from '../lib/queries'

interface Landed {
  code: string | null
  /** This install's bookmark key, which a bookmark saved before it had one lacks. */
  key: string | null
  signIn: GogSignIn | null
}

let landed: Landed | null = null

/** What the bookmark brought, read once per page load: the code leaves the
 *  address bar and the history entry, and the sign-in leaves storage, so
 *  neither can be used again. */
function land(): Landed {
  if (!landed) {
    const { hash } = window.location
    window.history.replaceState(null, '', window.location.pathname)
    landed = { code: codeFromHash(hash), key: keyFromHash(hash), signIn: takeSignIn() }
  }
  return landed
}

function problem({ code, key, signIn }: Landed) {
  if (!code) return 'No code came with the bookmark. Click it on GOG’s page after you sign in, the one whose address has code= in it.'
  if (!key)
    return 'This bookmark is from before Send to UpNext had a key of its own, so it no longer works. Drag Send to UpNext to your bookmarks bar again from Settings, then use the new one.'
  if (!signIn)
    return 'This browser didn’t start a GOG sign-in in the last ten minutes, or it was used already. Start again with Sign in on GOG, then use the bookmark.'
  return null
}

/** Where the "Send to UpNext" bookmark lands, with GOG's
 *  code in the address's fragment: it sends the code with the state this
 *  browser was given when it started the sign-in, then goes back to the
 *  wizard or Settings. */
export function GogCallbackPage() {
  const navigate = useNavigate()
  const qc = useQueryClient()
  const [here] = useState(land)
  const [error, setError] = useState<string | null>(() => problem(here))
  const back = here.signIn?.back ?? '/settings'
  const sent = useRef(false)

  useEffect(() => {
    const { code, key, signIn } = here
    // Once, even when React runs effects twice: the state works once.
    if (sent.current || !code || !key || !signIn) return
    sent.current = true
    api
      .gogCallback(signIn.state, code, key)
      .then((status) => {
        qc.setQueryData(keys.gog, status)
        const sep = signIn.back.includes('?') ? '&' : '?'
        navigate(`${signIn.back}${sep}gog=ok`, { replace: true })
      })
      .catch((e: unknown) => setError(e instanceof ApiError ? e.message : String(e)))
  }, [here, navigate, qc])

  return (
    <main className="flex min-h-screen items-center justify-center bg-ground px-4 py-10 text-ink">
      <div className="w-full max-w-[480px]">
        <div className="flex items-center gap-2.5">
          <PlayMark className="h-7 w-7" />
          <span className="font-display text-[22px] font-bold">UpNext</span>
        </div>
        <h1 className="mt-6 font-display text-[26px] leading-tight font-bold">{error ? 'GOG didn’t connect' : 'Connecting GOG…'}</h1>
        {error ? (
          <>
            <p className="mt-2 text-[15px] text-muted" role="alert">
              {error}
            </p>
            <Link to={back} replace className="btn-outline mt-6 inline-flex">
              Back to {back.startsWith('/setup') ? 'setup' : 'Settings'}
            </Link>
          </>
        ) : (
          <p className="mt-2 text-[15px] text-muted" aria-live="polite">
            Taking the sign-in from GOG’s page.
          </p>
        )}
      </div>
    </main>
  )
}
