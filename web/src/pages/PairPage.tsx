import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import type { PairAsked } from '../types'
import { LogoMark } from '../components/Shell'
import { pairCodeFromAddress } from '../lib/pairing'

/** How often the device asks whether the computer has answered. */
const ASK_EVERY_MS = 1500

/** Another device that isn't paired yet: the API answered 401, so
 *  this browser asks the computer UpNext runs on to allow it, and someone
 *  there taps Allow; or it takes the six-digit code from
 *  Settings there and trades it for its own cookie. A QR code from "Add a
 *  device" carries the code, so scanning it pairs with no typing. */
export function PairPage({ onPaired }: { onPaired: () => void }) {
  const scannedCode = pairCodeFromAddress()
  // Asking first, unless the QR code brought a code.
  const [typing, setTyping] = useState(() => scannedCode !== null)
  if (!typing) return <AskPage onPaired={onPaired} onTypeCode={() => setTyping(true)} />
  return <CodePage onPaired={onPaired} onAsk={() => setTyping(false)} />
}

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : e ? String(e) : null
}

function Frame({ children }: { children: ReactNode }) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-ground px-4 py-10 text-ink">
      <div className="w-full max-w-[440px]">
        <div className="flex items-center gap-2.5">
          <LogoMark className="h-7 w-7" />
          <span className="font-display text-[22px] font-bold">UpNext</span>
        </div>
        {children}
        <ProviderSignIn />
      </div>
    </main>
  )
}

/** How the last sign-in with the identity provider ended, when it didn't
 *  sign this browser in: the server sends it back to `/#signin_error=<why>`,
 *  or to `/#signin_waiting=<what next>` while the account waits to be
 *  confirmed on the computer. */
function signInOutcome(): { error: boolean; text: string } | null {
  const hash = new URLSearchParams(window.location.hash.slice(1))
  const error = hash.get('signin_error')
  if (error != null) return { error: true, text: `Signing in didn't work: ${error}` }
  const waiting = hash.get('signin_waiting')
  return waiting != null ? { error: false, text: `Signed in, but ${waiting}.` } : null
}

/** "Sign in with <provider>", when the identity provider
 *  is set up: a page load to the provider and back, which pairs this
 *  browser as the account's player. */
function ProviderSignIn() {
  const oidc = useQuery({ queryKey: ['oidc'], queryFn: api.oidc, retry: false })
  const [outcome] = useState(signInOutcome)
  // Read once, then off the address, so a reload doesn't show it again.
  useEffect(() => {
    if (outcome != null) window.history.replaceState(window.history.state, '', window.location.pathname + window.location.search)
  }, [outcome])
  if (!oidc.data?.enabled && outcome == null) return null
  return (
    <div className="mt-6 border-t border-line pt-6">
      {oidc.data?.enabled && (
        <a href="/api/v1/oidc/login" className="btn-outline w-full justify-center">
          Sign in with {oidc.data.name ?? 'your account'}
        </a>
      )}
      {outcome && (
        <p className={`mt-3 mb-0 text-[14px] ${outcome.error ? 'text-bad' : 'text-muted'}`} role={outcome.error ? 'alert' : 'status'}>
          {outcome.text}
        </p>
      )}
    </div>
  )
}

/** Ask the computer, and wait for its answer: the number shown here is
 *  the one the computer's dialog shows beside Allow. */
function AskPage({ onPaired, onTypeCode }: { onPaired: () => void; onTypeCode: () => void }) {
  const health = useQuery({ queryKey: ['health'], queryFn: api.health, retry: false })
  const container = health.data?.container === true
  const [asked, setAsked] = useState<PairAsked | null>(null)
  const [outcome, setOutcome] = useState<'denied' | 'expired' | null>(null)
  const ask = useMutation({
    mutationFn: api.askToPair,
    onSuccess: (a) => {
      setAsked(a)
      setOutcome(null)
    },
  })
  const finish = (why: 'denied' | 'expired') => {
    setOutcome(why)
    setAsked(null)
  }
  // The answer is acted on as it arrives: allowed opens UpNext, denied or
  // gone (the request expired) says so and offers to ask again.
  const answer = useQuery({
    queryKey: ['pair-answer', asked?.id],
    queryFn: async () => {
      try {
        const got = await api.pairAnswer(asked!)
        if (got.status === 'allowed') onPaired()
        else if (got.status === 'denied') finish('denied')
        return got
      } catch (e) {
        if (e instanceof ApiError && e.status === 404) finish('expired')
        throw e
      }
    },
    enabled: asked !== null,
    refetchInterval: ASK_EVERY_MS,
    refetchIntervalInBackground: true,
    retry: false,
  })
  const status = answer.data?.status
  const error = errText(ask.error)
  const waiting = asked !== null && status !== 'allowed'

  return (
    <Frame>
      <h1 className="mt-6 font-display text-[26px] font-bold leading-tight">Use UpNext on this device</h1>
      {waiting ? (
        <>
          <p className="mt-2 text-[15px] text-muted">
            {container ? (
              <>
                UpNext runs in a container: on its host, run{' '}
                <span className="font-mono text-[13px] break-all">docker exec upnext upnext pair approve {asked.match_code}</span>.
              </>
            ) : (
              <>
                On {asked.computer}, UpNext asks whether to allow this device. Choose <b className="font-semibold text-ink">Allow</b> if it shows
                this number:
              </>
            )}
          </p>
          <p className="mt-5 mb-0 text-center font-mono text-[44px] font-bold tracking-[0.3em]" aria-label={`Number ${asked.match_code.split('').join(' ')}`}>
            {asked.match_code}
          </p>
          <p className="mt-4 text-[13.5px] text-dim" role="status">
            Waiting for the computer…
          </p>
        </>
      ) : (
        <>
          <p className="mt-2 text-[15px] text-muted">
            {container
              ? 'UpNext runs in a container. Ask here, then allow this device with the command it shows, on the container’s host.'
              : 'Ask the computer UpNext runs on, and allow this device there with one tap. Nothing to type.'}
          </p>
          {outcome && (
            <p className="mt-3 mb-0 text-[14px] text-bad" role="alert">
              {outcome === 'denied' ? 'The computer said no.' : 'Nobody answered in time.'} You can ask again.
            </p>
          )}
          <button type="button" className="btn-accent mt-6 w-full" disabled={ask.isPending} onClick={() => ask.mutate()}>
            {ask.isPending ? 'Asking…' : 'Ask to be allowed'}
          </button>
          {error && (
            <p className="mt-3 mb-0 text-[14px] text-bad" role="alert">
              {error}
            </p>
          )}
        </>
      )}
      {window.location.pathname.startsWith('/setup') && (
        <p className="mt-3 text-[15px] text-muted">Setup carries on here once this device is in.</p>
      )}
      <button type="button" className="btn-quiet mt-6 px-0 text-[14px] text-dim underline hover:text-ink" onClick={onTypeCode}>
        Type a code instead
      </button>
      <p className="mt-6 text-[13px] text-dim">Once in, this browser stays signed in until the device is revoked in Settings.</p>
    </Frame>
  )
}

/** The six-digit code from Add a device, typed or from the QR code. */
function CodePage({ onPaired, onAsk }: { onPaired: () => void; onAsk: () => void }) {
  const [code, setCode] = useState(() => pairCodeFromAddress() ?? '')
  // In UpNext's container the code comes from its log.
  const health = useQuery({ queryKey: ['health'], queryFn: api.health, retry: false })
  const container = health.data?.container === true
  const pair = useMutation({ mutationFn: (c: string) => api.pair(c), onSuccess: onPaired })
  // Once, even when React runs effects twice in development: a code works once.
  const tried = useRef(false)
  useEffect(() => {
    const scanned = pairCodeFromAddress()
    if (scanned && !tried.current) {
      tried.current = true
      pair.mutate(scanned)
    }
  }, [pair])
  const digits = code.replace(/\D/g, '')
  const error = pair.error instanceof ApiError ? pair.error.message : pair.error ? String(pair.error) : null

  return (
    <Frame>
      <h1 className="mt-6 font-display text-[26px] font-bold leading-tight">Pair this device</h1>
      {container ? (
        <p className="mt-2 text-[15px] text-muted">
          UpNext runs in a container. Its log has a code when it starts (<span className="font-mono text-[13px]">docker logs upnext</span>
          ), and <span className="font-mono text-[13px] break-all">docker exec upnext upnext pair</span> makes a new one. Type the six digits.
        </p>
      ) : (
        <p className="mt-2 text-[15px] text-muted">
          On the computer UpNext runs on, open Settings › Devices and API, and choose <b className="font-semibold text-ink">Add a device</b>.
          Then type the six digits it shows.
        </p>
      )}
      {window.location.pathname.startsWith('/setup') && (
        <p className="mt-2 text-[15px] text-muted">Setup carries on here once this browser is paired.</p>
      )}
      <form
        className="mt-6 flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault()
          if (digits.length === 6 && !pair.isPending) pair.mutate(code)
        }}
      >
        <label className="flex flex-col gap-1.5 text-[13.5px] text-dim">
          Pairing code
          <input
            className="field h-14 text-center font-mono text-[28px] tracking-[0.35em]"
            inputMode="numeric"
            autoComplete="one-time-code"
            maxLength={7}
            placeholder="000000"
            value={code}
            // eslint-disable-next-line jsx-a11y/no-autofocus -- the page is only this field
            autoFocus
            onChange={(e) => setCode(e.target.value.replace(/[^\d ]/g, ''))}
          />
        </label>
        <button type="submit" className="btn-accent" disabled={digits.length !== 6 || pair.isPending}>
          {pair.isPending ? 'Pairing…' : 'Pair'}
        </button>
        {error && (
          <p className="m-0 text-[14px] text-bad" role="alert">
            {error}
          </p>
        )}
      </form>
      <button type="button" className="btn-quiet mt-6 px-0 text-[14px] text-dim underline hover:text-ink" onClick={onAsk}>
        Ask the computer instead
      </button>
      <p className="mt-6 text-[13px] text-dim">
        {container ? 'A code works once: the one in the log for an hour, a new one for five minutes.' : 'The code works once, for five minutes.'} Once paired, this browser stays signed in until the device is revoked in Settings.
      </p>
    </Frame>
  )
}
