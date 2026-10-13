import type { ReactNode } from 'react'
import type { Settings, SettingSource } from '../../types'
import { Checkbox, InlineField, Row, Status } from './parts'
import type { SaveFn } from './useSettingsSave'

/** Why a sign-in field can't be changed here, or null when it can. These
 *  decide who gets in, so the server takes them only from the computer
 *  UpNext runs on, and the environment wins over the app. */
function lockedBy(source: SettingSource, here: boolean): string | null {
  if (source === 'env') return 'Set in the environment.'
  if (!here) return 'Set only on the computer UpNext runs on, or in its environment.'
  return null
}

function Field({
  label,
  value,
  placeholder,
  source,
  here,
  secret,
  mono,
  onCommit,
}: {
  label: string
  value: string
  placeholder: string
  source: SettingSource
  here: boolean
  secret?: boolean
  mono?: boolean
  onCommit: (v: string) => Promise<string | null>
}) {
  const locked = lockedBy(source, here)
  return (
    <div className="flex flex-col gap-1">
      <div className="text-[13.5px] text-dim">{label}</div>
      <InlineField
        label={<span className="sr-only">{label}</span>}
        wrapClassName="max-w-[560px]"
        className={mono ? 'w-full font-mono text-[12.5px]' : 'w-full'}
        value={value}
        placeholder={placeholder}
        secret={secret}
        disabled={locked !== null}
        title={locked ?? undefined}
        onCommit={onCommit}
      />
    </div>
  )
}

function Note({ children }: { children: ReactNode }) {
  return <p className="m-0 mt-1.5 max-w-[760px] text-[13.5px] text-dim">{children}</p>
}

/** Settings › Devices and API, for the household's first player:
 *  signing in with an identity provider, and the proxies whose
 *  forwarding headers UpNext believes. Both are off until set. */
export function SignInRows({ s, save }: { s: Settings; save: SaveFn }) {
  const here = s.listen.this_computer
  const set = !!(s.oidc_issuer && s.oidc_client_id)
  const name = s.oidc_name || (s.oidc_issuer ? hostOf(s.oidc_issuer) : 'your provider')
  const locked = lockedBy(null, here)
  return (
    <>
      <Row
        name="Sign in with an identity provider"
        desc={
          !set ? (
            'Off: other devices pair with a code or a tap here'
          ) : s.oidc_redirect_uri ? (
            <Status tone="good">Other devices can sign in with {name}</Status>
          ) : (
            <Status tone="warn">Needs the address for other devices first</Status>
          )
        }
      >
        <div className="flex max-w-[760px] flex-col gap-2">
          <Field
            label="Issuer"
            value={s.oidc_issuer ?? ''}
            placeholder="https://sso.den.example.org"
            source={s.oidc_issuer_source}
            here={here}
            mono
            onCommit={(v) => save({ oidc_issuer: v || null }, v ? 'Identity provider saved' : 'Sign-in with a provider is off', { inline: true })}
          />
          <Field
            label="Client id"
            value={s.oidc_client_id ?? ''}
            placeholder="upnext"
            source={s.oidc_client_id_source}
            here={here}
            mono
            onCommit={(v) => save({ oidc_client_id: v || null }, 'Client id saved', { inline: true })}
          />
          <Field
            label="Client secret"
            value=""
            placeholder={s.oidc_client_secret_set ? 'Saved. Paste to replace' : 'None for a public client'}
            source={s.oidc_client_secret_source}
            here={here}
            secret
            mono
            onCommit={(v) => save({ oidc_client_secret: v }, 'Client secret saved', { inline: true })}
          />
          <Field
            label="Name on the button"
            value={s.oidc_name ?? ''}
            placeholder={s.oidc_issuer ? hostOf(s.oidc_issuer) : 'Authelia'}
            source={s.oidc_name_source}
            here={here}
            onCommit={(v) => save({ oidc_name: v || null }, 'Name saved', { inline: true })}
          />
          <Field
            label="Only these groups (optional, comma-separated)"
            value={s.oidc_allowed_groups ?? ''}
            placeholder="upnext"
            source={s.oidc_allowed_groups_source}
            here={here}
            onCommit={(v) => save({ oidc_allowed_groups: v || null }, v ? 'Groups saved' : 'Any account at the provider may sign in', { inline: true })}
          />
          <Checkbox
            checked={s.oidc_auto_provision}
            disabled={lockedBy(s.oidc_auto_provision_source, here) !== null}
            onChange={(v) => void save({ oidc_auto_provision: v }, v ? 'A new account becomes a new player' : 'New accounts need a player linked first')}
          >
            Make a new player for an account signing in for the first time
          </Checkbox>
        </div>
        <Note>
          {s.oidc_client_secret_set && s.oidc_client_secret_source === 'env' && 'The secret is set in the environment. '}
          At the provider, register UpNext as a client with the scopes openid and profile (and groups, with groups set) and this
          redirect address:{' '}
          {s.oidc_redirect_uri ? (
            <span className="font-mono text-[12px] break-all">{s.oidc_redirect_uri}</span>
          ) : (
            'set the address for other devices above, as https://, first'
          )}
          . A browser that signs in is paired as its player; to sign an account in as someone who's already here, use Link the next
          sign-in under Players, and confirm there that the account that signs in is theirs. {locked}
        </Note>
      </Row>

      <Row
        name="Trusted proxies"
        desc={s.trusted_proxies ? `Believed: ${s.trusted_proxies}` : 'None: forwarding headers mean nothing'}
      >
        <Field
          label="Their addresses"
          value={s.trusted_proxies ?? ''}
          placeholder="127.0.0.1, 10.0.0.2"
          source={s.trusted_proxies_source}
          here={here}
          mono
          onCommit={(v) => save({ trusted_proxies: v || null }, v ? 'Trusted proxies saved' : 'No proxy is trusted now', { inline: true })}
        />
        <div className="mt-2">
          <Checkbox
            checked={s.trusted_proxy_login}
            disabled={lockedBy(s.trusted_proxy_login_source, here) !== null || (!s.trusted_proxies && !s.trusted_proxy_login)}
            onChange={(v) => void save({ trusted_proxy_login: v }, v ? 'The proxy does the login' : 'Devices through the proxy need pairing again')}
          >
            The proxy does the login: its visitors need no pairing
          </Checkbox>
        </div>
        <Note>
          From these addresses UpNext takes the client's address, HTTPS and the host from the proxy's headers, and never who's playing.
          With the login on, anyone the proxy lets through lands on Who's playing, PINs as usual. Only for a proxy that really makes people
          sign in (Authelia, Authentik, oauth2-proxy) or a tailnet's <span className="font-mono text-[12px]">tailscale serve</span>, with
          UpNext reachable only through it; never with Tailscale Funnel or another proxy open to anyone.
        </Note>
      </Row>
    </>
  )
}

function hostOf(url: string): string {
  try {
    return new URL(url).host
  } catch {
    return url
  }
}
