import { useState, type FormEvent } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useLocation, useNavigate } from 'react-router'
import { api, ApiError } from '../api'
import type { NetworkConnection } from '../types'
import { LogoMark } from '../components/Shell'
import { useBigScreen } from '../lib/bigscreen'
import { canSwitch, missedComputer, openComputer } from '../lib/connect'
import { cx } from '../lib/format'
import { useThemeSync } from '../lib/theme'

/** How often the page asks whether the other computer answered. */
const ASK_EVERY_MS = 1500

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : e ? String(e) : null
}

/** "Use the library on another computer": the app
 *  window (`upnext app --connect`, or setup's first step) finds the
 *  computers on the network running UpNext, or takes an address, and asks
 *  one to allow this one; someone there taps Allow, and the window opens on
 *  that computer's UpNext. In a browser it lists them as links. */
export function ConnectPage() {
  useThemeSync()
  const big = useBigScreen()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { search } = useLocation()
  const app = canSwitch()
  const nearby = useQuery({ queryKey: ['nearby-computers'], queryFn: api.nearbyComputers, retry: false })
  const [typed, setTyped] = useState('')
  const [asking, setAsking] = useState<NetworkConnection | null>(null)
  const [outcome, setOutcome] = useState<string | null>(null)

  const connect = useMutation({
    mutationFn: ({ address, id }: { address: string; id?: string }) => api.connectTo(address, id ?? null),
    onSuccess: (c) => {
      setAsking(c)
      setOutcome(null)
    },
  })
  // The answer is acted on as it arrives: allowed opens the window there,
  // anything but waiting says what happened and stops asking.
  const answer = useQuery({
    queryKey: ['connection', asking?.address, asking?.match_code],
    queryFn: async () => {
      const got = await api.connection()
      if (asking && got.status !== 'waiting') {
        if (got.status === 'allowed' && got.id) {
          void qc.invalidateQueries({ queryKey: ['nearby-computers'] })
          if (app) openComputer(got.id)
          else setOutcome(`${asking.computer} allowed this computer.`)
        } else {
          setOutcome(got.status === 'denied' ? `${asking.computer} said no.` : `Nobody answered on ${asking.computer} in time.`)
        }
        setAsking(null)
      }
      return got
    },
    enabled: asking !== null,
    refetchInterval: ASK_EVERY_MS,
    refetchIntervalInBackground: true,
    retry: false,
  })
  const forget = useMutation({
    mutationFn: api.forgetComputer,
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['nearby-computers'] }),
  })

  const stopAsking = () => {
    void api.stopConnecting()
    setAsking(null)
  }
  const thisComputer = () => {
    // The window forgets the other computer as the one it opens on.
    if (app) openComputer(null)
    else navigate('/')
  }
  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (typed.trim() && !connect.isPending) connect.mutate({ address: typed.trim() })
  }

  const data = nearby.data
  const missing = missedComputer(search)
  const missed = missing ? data?.saved.find((c) => c.id === missing.id) : undefined
  const saved = data?.saved ?? []
  const found = (data?.found ?? []).filter((f) => !saved.some((s) => s.id === f.id))
  const error = errText(connect.error) ?? errText(answer.error) ?? errText(forget.error)
  const text = big ? 'text-[1rem]' : 'text-[15px]'
  const small = big ? 'text-[0.85rem]' : 'text-[13.5px]'
  const btn = big ? 'big-btn-outline' : 'btn-outline'
  const accent = big ? 'big-btn-accent' : 'btn-accent'

  return (
    <main className="flex min-h-screen justify-center bg-ground px-4 py-10 text-ink">
      <div className={cx('w-full', big ? 'max-w-[40rem]' : 'max-w-[560px]')}>
        <div className="flex items-center gap-2.5">
          <LogoMark className="h-7 w-7" />
          <span className="font-display text-[22px] font-bold">UpNext</span>
        </div>
        <h1 className={cx('mt-6 font-display font-bold leading-tight', big ? 'text-[2rem]' : 'text-[26px]')}>Use the library on another computer</h1>
        <p className={cx('mt-2 text-muted', text)}>
          {app
            ? 'Choose the computer whose UpNext this window should show. It asks there, and someone allows it with a tap. Games still start on this computer.'
            : 'These computers run UpNext on your network. Open one, and ask it to allow this browser.'}
        </p>

        {missed && (
          <div className="mt-5 rounded-xl border border-line-strong bg-sheet p-4" role="alert">
            {missing?.changed ? (
              <>
                <p className={cx('m-0 font-semibold', text)}>{missed.name} isn’t at {missed.address} any more</p>
                <p className={cx('m-0 mt-1 text-muted', small)}>
                  Another computer running UpNext answers there now, so this window didn’t open it or send it {missed.name}’s access. Find{' '}
                  {missed.name} again below, or forget it.
                </p>
              </>
            ) : (
              <>
                <p className={cx('m-0 font-semibold', text)}>{missed.name} didn’t answer</p>
                <p className={cx('m-0 mt-1 text-muted', small)}>
                  Nothing answered at {missed.address}. Is it on, with Allow other devices on? Its address may have changed: find it again below.
                </p>
              </>
            )}
            <div className="mt-3 flex flex-wrap gap-2.5">
              <button type="button" className={accent} data-focus-first="" onClick={() => openComputer(missed.id)}>
                Try again
              </button>
              <button type="button" className={btn} onClick={thisComputer}>
                Use this computer’s library
              </button>
            </div>
          </div>
        )}

        {asking ? (
          <section className="mt-6 rounded-xl border border-line-strong bg-sheet p-5" aria-live="polite">
            <p className={cx('m-0', text)}>
              On <b className="font-semibold">{asking.computer}</b>, UpNext asks whether to allow this computer. Choose <b className="font-semibold">Allow</b> if it shows:
            </p>
            <p className="m-0 mt-4 text-center font-mono text-[44px] font-bold tracking-[0.3em]" aria-label={`Number ${asking.match_code.split('').join(' ')}`}>
              {asking.match_code}
            </p>
            <p className={cx('m-0 mt-3 text-dim', small)} role="status">
              Waiting for {asking.computer}…
            </p>
            <button type="button" className={cx(btn, 'mt-4')} data-focus-first="" onClick={stopAsking}>
              Stop asking
            </button>
          </section>
        ) : (
          <>
            {outcome && (
              <p className={cx('mt-5 mb-0', text)} role="status">
                {outcome}
              </p>
            )}
            {saved.length > 0 && (
              <section className="mt-6">
                <h2 className={cx('m-0 font-semibold', text)}>Allowed before</h2>
                <ul className="m-0 mt-2 flex list-none flex-col gap-2 p-0" data-focus-group>
                  {saved.map((c, i) => (
                    <li key={c.id} className="flex flex-wrap items-center gap-3 rounded-xl border border-line p-3">
                      <div className="min-w-0 flex-1">
                        <div className={cx('font-semibold', text)}>{c.name}</div>
                        <div className={cx('text-dim break-all', small)}>{c.address}</div>
                      </div>
                      {app ? (
                        <button type="button" className={accent} data-focus-first={i === 0 && !missed ? '' : undefined} onClick={() => openComputer(c.id)}>
                          Open
                        </button>
                      ) : (
                        <a className={accent} href={c.address}>
                          Open
                        </a>
                      )}
                      <button type="button" className={btn} disabled={forget.isPending} onClick={() => forget.mutate(c.id)}>
                        Forget
                      </button>
                    </li>
                  ))}
                </ul>
                <p className={cx('m-0 mt-2 text-dim', small)}>Forgetting one drops its access here; revoke it there too, in Settings › Devices and API.</p>
              </section>
            )}

            <section className="mt-6">
              <h2 className={cx('m-0 font-semibold', text)}>On your network</h2>
              {nearby.isPending ? (
                <p className={cx('m-0 mt-2 text-dim', small)} role="status">
                  Looking…
                </p>
              ) : found.length === 0 ? (
                <p className={cx('m-0 mt-2 text-dim', small)}>
                  None found. On the other computer, turn on Allow other devices in Settings › Devices and API; or type its address below.
                </p>
              ) : (
                <ul className="m-0 mt-2 flex list-none flex-col gap-2 p-0" data-focus-group>
                  {found.map((c, i) => (
                    <li key={c.id} className="flex flex-wrap items-center gap-3 rounded-xl border border-line p-3">
                      <div className="min-w-0 flex-1">
                        <div className={cx('font-semibold', text)}>{c.name}</div>
                        <div className={cx('text-dim break-all', small)}>
                          {c.address}
                          {c.version ? ` · UpNext ${c.version}` : ''}
                        </div>
                      </div>
                      {app ? (
                        <button
                          type="button"
                          className={accent}
                          data-focus-first={i === 0 && saved.length === 0 && !missed ? '' : undefined}
                          disabled={connect.isPending}
                          onClick={() => connect.mutate({ address: c.address, id: c.id })}
                        >
                          Ask to use it
                        </button>
                      ) : (
                        <a className={accent} href={c.address}>
                          Open
                        </a>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              <button type="button" className={cx('btn-quiet mt-2 px-0 underline', small)} onClick={() => void nearby.refetch()} disabled={nearby.isFetching}>
                {nearby.isFetching ? 'Looking…' : 'Look again'}
              </button>
            </section>

            {app && (
              <form className="mt-6 flex flex-col gap-2" onSubmit={submit}>
                <label className={cx('flex flex-col gap-1.5 text-dim', small)}>
                  Or its address
                  <input
                    className="field h-11"
                    value={typed}
                    placeholder="den.lan or 192.168.0.42:8787"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    onChange={(e) => setTyped(e.target.value)}
                  />
                </label>
                <button type="submit" className={cx(btn, 'self-start')} disabled={!typed.trim() || connect.isPending}>
                  {connect.isPending ? 'Asking…' : 'Ask to use it'}
                </button>
                <p className={cx('m-0 text-dim', small)}>
                  Off your home network, use the address it has there, such as https://den.tailnet.ts.net behind tailscale serve.
                </p>
              </form>
            )}
          </>
        )}

        {error && (
          <p className={cx('mt-4 mb-0 text-bad', small)} role="alert">
            {error}
          </p>
        )}

        <div className="mt-8 border-t border-line pt-5">
          <button type="button" className={btn} onClick={thisComputer}>
            Use this computer’s library
          </button>
        </div>
      </div>
    </main>
  )
}
