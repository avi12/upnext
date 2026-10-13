import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api, ApiError } from '../../api'
import type { NewToken, Pairing, Settings, TokenScope } from '../../types'
import { useToast } from '../useToast'
import { absoluteTime, cx, relativeTime } from '../../lib/format'
import { Checkbox, InlineField, Row, Section, Segmented, Status, TextButton } from './parts'
import type { SaveFn } from './useSettingsSave'
import { canSwitch, openComputer, remoteName } from '../../lib/connect'
import { pinOk, possessive, usePlayers } from '../../lib/players'
import { PinField } from '../players/PinField'
import { SignInRows } from './SignIn'

/** Tokens and pairing codes are made only on the computer UpNext runs on,
 *  so a token or a paired device can't hand out more access. */
const ONLY_HERE = 'Only on the computer UpNext runs on.'

/** In UpNext's container nobody is that computer: a terminal on
 *  the host makes codes and tokens. */
function OnlyHere({ container, what }: { container: boolean; what: 'pair' | 'token create --name NAME --scope read|full' }) {
  if (!container) return <p className="m-0 text-[13.5px] text-dim">{ONLY_HERE}</p>
  return (
    <p className="m-0 text-[13.5px] text-dim">
      UpNext runs in a container, so a terminal on its host makes {what === 'pair' ? 'codes' : 'tokens'}:{' '}
      <span className="font-mono text-[12px] break-all">docker exec upnext upnext {what}</span>
    </p>
  )
}

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

const SCOPES: { value: TokenScope; label: string }[] = [
  { value: 'read', label: 'Read' },
  { value: 'full', label: 'Full' },
]

const SCOPE_TEXT: Record<TokenScope, string> = {
  read: 'Read: looks, changes nothing',
  full: 'Full: everything, picks and settings too',
}

/** Settings › Devices and API: the address other devices use, and
 *  the tokens that let a script or another device use the API. */
export function DevicesSection({ s, save }: { s: Settings; save: SaveFn }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [name, setName] = useState('')
  const [scope, setScope] = useState<TokenScope>('read')
  const [made, setMade] = useState<NewToken | null>(null)
  const [revoking, setRevoking] = useState<number | null>(null)
  // With two or more players, a token can act for one of them alone.
  const { players, current, bound, multi } = usePlayers()
  // Who gets in is the household's first player's to set.
  const first = !bound && current != null && players.every((p) => p.id >= current.id)
  const [forPlayer, setForPlayer] = useState('anyone')
  const playerName = (id: number | null | undefined) => players.find((p) => p.id === id)?.name
  // Bound to a player with a PIN, the token acts as them without one, so it takes theirs.
  const pinned = players.find((p) => String(p.id) === forPlayer && p.has_pin)
  const [tokenPin, setTokenPin] = useState('')

  const tokens = useQuery({ queryKey: ['tokens'], queryFn: api.tokens })
  const create = useMutation({
    mutationFn: () => api.createToken(name.trim(), scope, forPlayer === 'anyone' ? null : Number(forPlayer), pinned ? tokenPin : null),
    onSuccess: (t) => {
      setMade(t)
      setName('')
      setTokenPin('')
      qc.invalidateQueries({ queryKey: ['tokens'] })
    },
    onError: (e) => toast.error("Couldn't make the token", errText(e)),
  })
  const revoke = useMutation({
    mutationFn: api.revokeToken,
    onSuccess: (_, id) => {
      setRevoking(null)
      if (made?.id === id) setMade(null)
      qc.invalidateQueries({ queryKey: ['tokens'] })
      toast.success('Revoked', 'It stopped working at once')
    },
    onError: (e) => toast.error("Couldn't revoke it", errText(e)),
  })

  // A browser signed in with the identity provider signs itself out: its
  // token goes, and the app asks to pair or sign in again.
  const signOut = useMutation({
    mutationFn: api.oidcLogout,
    onSuccess: () => window.dispatchEvent(new Event('upnext:unpaired')),
    onError: (e) => toast.error("Couldn't sign out", errText(e)),
  })

  const list = tokens.data?.tokens ?? []
  const listen = `${s.listen.host}:${s.listen.port}`

  return (
    <Section id="devices" title="Devices and API" desc="This computer needs nothing. A phone pairs; scripts use a token.">
      <Row
        name="Allow other devices"
        desc={
          s.listen.other_devices
            ? `Listening on ${listen}: your network too`
            : `Listening on ${listen}: only this computer, or a proxy in front`
        }
      >
        <Checkbox
          checked={s.listen.other_devices}
          disabled={s.listen.forced}
          onChange={(v) => void save({ allow_other_devices: v }, v ? 'Other devices can reach UpNext' : 'Only this computer now')}
          note={s.listen.forced ? 'Set by UPNEXT_HOST, so it stays on.' : undefined}
        >
          Listen on the network, not just this computer
        </Checkbox>
        {s.listen.error && (
          <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-bad" role="alert">
            {s.listen.error}
          </p>
        )}
        <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">
          Nothing from another device works until it's paired or has a token. Over plain HTTP, UpNext takes them only from your own network.
        </p>
      </Row>

      <Found s={s} save={save} />

      <ThisWindow here={s.listen.this_computer} />

      <AddDevice here={s.listen.this_computer} container={s.listen.container} />

      <Row
        name="Address for other devices"
        desc="Optional: the name and port they type, or your proxy's https:// address"
      >
        <InlineField
          label={<span className="sr-only">Address for other devices</span>}
          wrapClassName="max-w-[460px]"
          className="w-full"
          value={s.public_url ?? ''}
          placeholder="http://upnext.lan:8787"
          disabled={s.public_url_source === 'env'}
          onCommit={(v) => save({ public_url: v || null }, v ? `Other devices use ${v}` : 'Address cleared', { inline: true })}
        />
        <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">
          {s.public_url_source === 'env' && 'Set by UPNEXT_PUBLIC_URL. '}
          UpNext answers requests addressed to it, Steam sign-in comes back to it, and Add a device shows it. Left empty, a device with a
          token can use whatever address reaches UpNext.
        </p>
      </Row>

      <Row
        name="HTTPS"
        desc={
          s.listen.https ? (
            <Status tone="good">On, on the same port as plain HTTP</Status>
          ) : (
            'Off: plain HTTP, for your own network only'
          )
        }
      >
        <div className="flex max-w-[760px] flex-col gap-1">
          <div className="text-[13.5px] text-dim">Certificate (PEM, with its chain)</div>
          <InlineField
            label={<span className="sr-only">Certificate (PEM, with its chain)</span>}
            wrapClassName="max-w-[560px]"
            className="w-full font-mono text-[12.5px]"
            value={s.tls_cert ?? ''}
            placeholder="/etc/ssl/upnext/fullchain.pem"
            onCommit={(v) => save({ tls_cert: v || null }, v ? 'Certificate saved' : 'Certificate cleared', { inline: true })}
          />
          <div className="text-[13.5px] text-dim">Private key</div>
          <InlineField
            label={<span className="sr-only">Private key</span>}
            wrapClassName="max-w-[560px]"
            className="w-full font-mono text-[12.5px]"
            value={s.tls_key ?? ''}
            placeholder="/etc/ssl/upnext/privkey.pem"
            onCommit={(v) => save({ tls_key: v || null }, v ? 'Key saved' : 'Key cleared', { inline: true })}
          />
        </div>
        <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">
          Files on the computer UpNext runs on, set only there. Or skip this and put a proxy in front:{' '}
          <span className="font-mono text-[12px]">tailscale serve</span> gives a trusted certificate with no setup (docs/other-devices.md, "HTTPS and
          proxies").
        </p>
      </Row>

      {first && <SignInRows s={s} save={save} />}

      <div className="border-b border-line py-3.5">
        <div className="text-[15.5px] font-bold">Make a token</div>
        <div className="mt-0.5 text-[13.5px] text-dim">
          For a script, a dashboard or another device: it sends <span className="font-mono text-[12px]">Authorization: Bearer upnext_…</span>.
        </div>
        {!s.listen.this_computer && (
          <div className="mt-2">
            <OnlyHere container={s.listen.container} what="token create --name NAME --scope read|full" />
          </div>
        )}
        <form
          className={`mt-3 flex-wrap items-end gap-x-3 gap-y-2 ${s.listen.this_computer ? 'flex' : 'hidden'}`}
          onSubmit={(e) => {
            e.preventDefault()
            if (name.trim() && !create.isPending && (!pinned || pinOk(tokenPin))) create.mutate()
          }}
        >
          <label className="flex min-w-0 flex-1 basis-[220px] flex-col gap-1 text-[13.5px] text-dim">
            What it's for
            <input className="field text-sm" value={name} maxLength={60} placeholder="phone, Home Assistant" onChange={(e) => setName(e.target.value)} />
          </label>
          <Segmented label="Scope" options={SCOPES} value={scope} onChange={setScope} />
          {multi && (
            <label className="flex flex-col gap-1 text-[13.5px] text-dim">
              Acts for
              <select
                className="field text-sm"
                value={forPlayer}
                onChange={(e) => {
                  setForPlayer(e.target.value)
                  setTokenPin('')
                }}
              >
                <option value="anyone">Whoever it names, else the default player</option>
                {players.map((p) => (
                  <option key={p.id} value={String(p.id)}>
                    {p.name} alone
                  </option>
                ))}
              </select>
            </label>
          )}
          {pinned && <PinField name={pinned.name} value={tokenPin} onChange={setTokenPin} />}
          <button type="submit" className="btn-outline" disabled={!name.trim() || create.isPending || (!!pinned && !pinOk(tokenPin))}>
            {create.isPending ? 'Making…' : 'Make token'}
          </button>
          <p className="m-0 w-full text-[13.5px] text-dim">{SCOPE_TEXT[scope]}. Picks need Full: each one is an AI call.</p>
        </form>
        {made && <Made token={made} onDone={() => setMade(null)} />}
      </div>

      {tokens.isError && <p className="py-3 text-sm text-bad">Couldn't load the tokens: {errText(tokens.error)}</p>}
      {tokens.data && list.length === 0 && <p className="m-0 border-b border-line py-3 text-sm text-dim">No tokens yet.</p>}
      {list.length > 0 && (
        <ul className="m-0 list-none p-0" aria-label="Tokens">
          {list.map((t) => (
            <li key={t.id} className="border-b border-line py-3">
              <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
                <div className="min-w-0 flex-1 basis-[300px]">
                  <div className="text-[15px]">
                    <span className="font-semibold">{t.name}</span>
                    <span className="text-muted"> · {t.scope === 'full' ? 'Full' : 'Read'}</span>
                    {t.kind === 'device' && <span className="text-muted"> · paired device</span>}
                    {t.kind === 'oidc' && <span className="text-muted"> · signed in with the identity provider</span>}
                    {t.player_id != null && playerName(t.player_id) && (
                      <span className="text-muted"> · {possessive(playerName(t.player_id) ?? '')}</span>
                    )}
                    {tokens.data?.current === t.id && <span className="text-accent"> · this device</span>}
                  </div>
                  <div className="text-[13.5px] text-dim">
                    <span className="font-mono text-[12px]">{t.hint}…</span>
                    {' · made '}
                    <span title={absoluteTime(t.created_at)}>{relativeTime(t.created_at)}</span>
                    {' · '}
                    {t.last_used_at ? (
                      <span title={absoluteTime(t.last_used_at)}>used {relativeTime(t.last_used_at)}</span>
                    ) : (
                      'never used'
                    )}
                  </div>
                </div>
                {t.kind === 'oidc' && tokens.data?.current === t.id ? (
                  <TextButton onClick={() => signOut.mutate()} disabled={signOut.isPending}>
                    Sign out
                  </TextButton>
                ) : (
                  <TextButton aria-expanded={revoking === t.id} aria-label={`Revoke ${t.name}`} onClick={() => setRevoking((r) => (r === t.id ? null : t.id))}>
                    Revoke
                  </TextButton>
                )}
              </div>
              {revoking === t.id && (
                <div className="flex flex-col gap-2.5 pt-3">
                  <p className="m-0 max-w-[760px] text-[14px] text-muted">
                    Revoke {t.name}? Whatever uses it stops working at once{tokens.data?.current === t.id ? ', this device included' : ''}.
                  </p>
                  <div className="flex items-center gap-2">
                    <button type="button" className="btn-outline text-bad" disabled={revoke.isPending} onClick={() => revoke.mutate(t.id)}>
                      {revoke.isPending ? 'Revoking…' : 'Revoke'}
                    </button>
                    <TextButton onClick={() => setRevoking(null)}>Cancel</TextButton>
                  </div>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      <p className="m-0 pt-3 text-[13.5px] text-dim">
        Tokens work from the terminal too: <span className="font-mono text-[12px]">upnext token create --name phone --scope read</span>,{' '}
        <span className="font-mono text-[12px]">upnext token list</span> and <span className="font-mono text-[12px]">upnext token revoke</span>.
      </p>
    </Section>
  )
}

/** A token just made, shown this once, with Copy. */
function Made({ token, onDone }: { token: NewToken; onDone: () => void }) {
  const field = useRef<HTMLInputElement>(null)
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(token.token)
      setCopied(true)
    } catch {
      // No clipboard (an http:// address that isn't localhost): select it to copy by hand.
      field.current?.select()
    }
  }
  return (
    <div className="mt-3 flex flex-col gap-2 rounded-sm border border-line p-3" role="status">
      <p className="m-0 text-[14px]">
        <b className="font-semibold">{token.name}</b>
        <span className="text-muted">: copy it now. UpNext keeps only a fingerprint of it, so it can't show it again.</span>
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <input
          ref={field}
          readOnly
          aria-label={`Token for ${token.name}`}
          className="field min-w-0 flex-1 basis-[320px] font-mono text-[12.5px]"
          value={token.token}
          onFocus={(e) => e.target.select()}
        />
        <button type="button" className={cx('btn-outline', copied && 'text-good')} onClick={() => void copy()}>
          {copied ? 'Copied' : 'Copy'}
        </button>
        <TextButton onClick={onDone}>Done</TextButton>
      </div>
    </div>
  )
}

/** Seconds left until `at` (Unix seconds), ticking. */
function useSecondsLeft(at: number | null) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    if (at == null) return
    const t = window.setInterval(() => setNow(Date.now() / 1000), 1000)
    return () => window.clearInterval(t)
  }, [at])
  return at == null ? 0 : Math.max(0, Math.round(at - now))
}

/** Add a device: a six-digit code for five minutes, with a QR code of the
 *  address the phone opens. It notices when the code has been used. */
/** Found on the network: whether UpNext announces
 *  itself, so a phone or another computer finds it without an address, and
 *  the name it goes by there. */
function Found({ s, save }: { s: Settings; save: SaveFn }) {
  const desc = !s.announce
    ? 'Off: other devices need the address or the QR code'
    : s.listen.other_devices
      ? `Announced as “${s.network_name_shown}”`
      : 'Once other devices are allowed'
  return (
    <Row name="Found on your network" desc={desc}>
      <Checkbox checked={s.announce} onChange={(v) => void save({ announce: v }, v ? 'Announced on your network' : 'No longer announced')}>
        Announce UpNext on the network, so phones and other computers find it
      </Checkbox>
      <div className="mt-2 flex max-w-[760px] flex-col gap-1">
        <div className="text-[13.5px] text-dim">Name on the network</div>
        <InlineField
          label={<span className="sr-only">Name on the network</span>}
          wrapClassName="max-w-[360px]"
          className="w-full"
          value={s.network_name ?? ''}
          placeholder={s.network_name_shown}
          maxLength={63}
          onCommit={(v) => save({ network_name: v || null }, v ? `Called “${v}” on the network` : 'Called by its host name again', { inline: true })}
        />
      </div>
      <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">
        A device that finds it asks to be allowed, and this computer shows Allow and Deny. Only the name, the port and UpNext's version are
        announced, nothing about your library.
      </p>
    </Row>
  )
}

/** In the app window: the library it shows, this computer's or another
 *  computer's, and the way to the other. */
function ThisWindow({ here }: { here: boolean }) {
  const remote = remoteName()
  if (remote) {
    return (
      <Row name="This window" desc={`Shows the library on ${remote}`}>
        <button type="button" className="btn-outline" onClick={() => openComputer(null)}>
          Use this computer's library
        </button>
        <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">Games still start on the computer this window is on.</p>
      </Row>
    )
  }
  if (!canSwitch() || !here) return null
  return (
    <Row name="This window" desc="Shows this computer's library">
      <Link to="/connect" className="btn-outline">
        Use the library on another computer
      </Link>
      <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">
        A handheld or a second PC can show the library on your main computer instead of its own.
      </p>
    </Row>
  )
}

function AddDevice({ here, container }: { here: boolean; container: boolean }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [shown, setShown] = useState<Pairing | null>(null)
  const left = useSecondsLeft(shown?.expires_at ?? null)
  // Whose device: one player's, preselected as whoever is playing
  // here, or everyone's, which asks who's playing like this computer.
  const { players, current, multi } = usePlayers()
  const [owner, setOwner] = useState<string | null>(null)
  const whose = owner ?? (current ? String(current.id) : 'shared')
  // A device that will be a PIN player's takes their PIN, as switching to them does.
  const pinned = multi ? players.find((p) => String(p.id) === whose && p.has_pin) : undefined
  const [pin, setPin] = useState('')
  const start = useMutation({
    mutationFn: () => api.startPairing(undefined, multi ? (whose === 'shared' ? 'shared' : Number(whose)) : undefined, pinned ? pin : null),
    onSuccess: (p) => {
      setShown(p)
      setPin('')
    },
    onError: (e) => toast.error("Couldn't make a code", errText(e)),
  })
  const end = useMutation({ mutationFn: api.endPairing, onSettled: () => setShown(null) })
  // While a code is up, look every few seconds whether it was used: gone
  // before it ran out means a device paired.
  const code = shown?.code ?? null
  const expiresAt = shown?.expires_at ?? 0
  useEffect(() => {
    if (code == null) return
    const look = window.setInterval(() => {
      if (Date.now() / 1000 > expiresAt - 1) return
      api
        .pairing()
        .then((now) => {
          if (now?.code === code) return
          setShown(null)
          if (now === null) {
            qc.invalidateQueries({ queryKey: ['tokens'] })
            toast.success('Paired', 'The new device is in the list below')
          }
        })
        .catch(() => {})
    }, 3000)
    return () => window.clearInterval(look)
  }, [code, expiresAt, qc, toast])
  const expired = shown != null && left === 0

  return (
    <div className="border-b border-line py-3.5">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <div className="min-w-0 flex-1 basis-[300px]">
          <div className="text-[15.5px] font-bold">Add a device</div>
          <div className="mt-0.5 text-[13.5px] text-dim">A phone or another computer's browser: open the address on it, then type the code.</div>
        </div>
        {!here && <OnlyHere container={container} what="pair" />}
        {here && !shown && (
          <button type="button" className="btn-outline" disabled={start.isPending || (!!pinned && !pinOk(pin))} onClick={() => start.mutate()}>
            {start.isPending ? 'Making a code…' : 'Add a device'}
          </button>
        )}
      </div>
      {here && !shown && multi && (
        <div className="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <Segmented
            label="Whose device is it?"
            options={[...players.map((p) => ({ value: String(p.id), label: p.name })), { value: 'shared', label: 'Everyone’s' }]}
            value={whose}
            onChange={(o) => {
              setOwner(o)
              setPin('')
            }}
          />
          <span className="text-[13.5px] text-dim">
            {whose === 'shared' ? 'Everyone’s: it asks who’s playing, like this computer.' : 'It acts for them alone and can’t switch.'}
          </span>
          {pinned && <PinField className="w-full" name={pinned.name} value={pin} onChange={setPin} />}
        </div>
      )}
      {shown && (
        <div className="mt-3 flex flex-wrap items-start gap-5 rounded-sm border border-line p-4" role="status">
          {shown.qr_svg && !expired && (
            <img
              className="h-[168px] w-[168px] shrink-0 rounded-sm bg-white p-1.5"
              alt={`QR code for ${shown.address}`}
              src={`data:image/svg+xml;utf8,${encodeURIComponent(shown.qr_svg)}`}
            />
          )}
          <div className="flex min-w-0 flex-1 basis-[240px] flex-col gap-2">
            {shown.address ? (
              <p className="m-0 text-[14px]">
                <span className="text-muted">On the device, open </span>
                <span className="font-mono text-[13px] break-all">{shown.address}</span>
              </p>
            ) : (
              <p className="m-0 text-[14px] text-warn">
                UpNext only listens on this computer: turn on Allow other devices above, or set the address other devices use.
              </p>
            )}
            {expired ? (
              <p className="m-0 text-[14px] text-dim">The code ran out.</p>
            ) : (
              <>
                <div className="font-mono text-[34px] font-bold tracking-[0.3em]" aria-label={`Code ${shown.code.split('').join(' ')}`}>
                  {shown.code}
                </div>
                <p className="m-0 text-[13.5px] text-dim">
                  Works once, for {Math.floor(left / 60)}:{String(left % 60).padStart(2, '0')} more. Five wrong tries end it.
                </p>
              </>
            )}
            <div className="flex items-center gap-2">
              {expired ? (
                <button type="button" className="btn-outline" onClick={() => start.mutate()}>
                  New code
                </button>
              ) : (
                <TextButton onClick={() => end.mutate()}>Cancel</TextButton>
              )}
              {expired && <TextButton onClick={() => setShown(null)}>Done</TextButton>}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
