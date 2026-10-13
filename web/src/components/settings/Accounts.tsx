import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Settings, SetupInfo, SteamTestResult } from '../../types'
import { useToast } from '../useToast'
import { cx, plural } from '../../lib/format'
import { Ext, InlineField, Row, RowGrid, Section, Status, TextButton } from './parts'
import { everyones, usePlayers } from '../../lib/players'
import { steamIdText } from './accountText'
import { GogBookmark } from '../setup/GogBookmark'
import { OtherUsersFolders } from '../setup/OtherUsersFolders'
import { GalaxyStoresRow } from './GalaxyStores'
import { ByHandRow } from './ByHand'
import { EpicRow } from './Epic'
import type { SaveFn } from './useSettingsSave'
import { keys, useGog } from '../../lib/queries'

export function AccountsSection({ s, save }: { s: Settings; save: SaveFn }) {
  return (
    <Section id="accounts" title="Accounts" desc="Steam or Heroic is enough to start. The rest make picks better.">
      <RowGrid>
        <SteamRow s={s} save={save} />
        <GogRow noWindow={s.listen.container || !s.listen.this_computer} back="/settings" />
        <HeroicRow s={s} />
        <EpicRow launcherDir={s.epic_launcher_dir} />
        <RawgRow s={s} save={save} />
        <IgdbRow s={s} save={save} />
        <GalaxyRow s={s} save={save} />
        <GalaxyStoresRow s={s} save={save} />
        <ByHandRow />
      </RowGrid>
    </Section>
  )
}

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** Steam's sign-in, Web API key and SteamID. `inWizard`: open from the
 *  start, and the sign-in comes back to the setup wizard, or with
 *  `then="player"` to a player's own setup. */
export function SteamRow({
  s,
  save,
  inWizard = false,
  then = 'setup',
}: {
  s: Settings
  save: SaveFn
  inWizard?: boolean
  then?: 'setup' | 'player'
}) {
  const [open, setOpen] = useState(inWizard)
  const [test, setTest] = useState<{ ok: SteamTestResult } | { error: string } | null>(null)
  const steamTest = useMutation({
    mutationFn: api.steamTest,
    onSuccess: (r) => setTest({ ok: r }),
    onError: (e) => setTest({ error: errText(e) }),
  })
  const tested = test && 'ok' in test
  const keyText = tested ? 'API key works' : s.steam_api_key_set ? 'key saved' : 'no API key'

  // Who the account is goes on a line of its own under the row, the row's
  // full width: beside Change it ran over the name, since a persona can be
  // any length (Windows retest). The right side keeps the short key status.
  const who = steamIdText(s)
  const status = s.steam_id ? (
    <Status
      tone={s.steam_api_key_set ? 'good' : 'warn'}
      title={`${who} · ${keyText}${s.steam_api_key_set ? '' : '. Without a Web API key, games come from the Steam client’s files only'}`}
    >
      {tested ? 'API key works' : s.steam_api_key_set ? 'Key saved' : 'No API key'}
    </Status>
  ) : (
    <Status tone="dim">Not signed in</Status>
  )

  return (
    <Row
      name="Steam"
      desc="Library, playtime and installs"
      right={
        <>
          {status}
          <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            {open ? 'Done' : s.steam_id ? 'Change' : 'Set up'}
          </TextButton>
        </>
      }
    >
      {s.steam_id && (
        <div className={cx('-mt-2 flex min-w-0 items-center gap-2 text-[13.5px] text-muted', open && 'mb-3')} title={who}>
          {s.steam_avatar && <img src={s.steam_avatar} alt="" className="h-[18px] w-[18px] shrink-0 rounded-full object-cover" />}
          <span className="truncate">{who}</span>
        </div>
      )}
      {open && (
        <div className="space-y-3 pb-1">
          <div className="flex flex-wrap items-center gap-3">
            <a href={inWizard ? `/api/v1/steam/login?then=${then}` : '/api/v1/steam/login'} className="btn-outline">
              {s.steam_id ? 'Sign in with a different account' : 'Sign in with Steam'}
            </a>
            <button
              type="button"
              className="btn-outline"
              disabled={steamTest.isPending || !s.steam_id || !s.steam_api_key_set}
              title={!s.steam_api_key_set ? 'Paste a Web API key below first' : undefined}
              onClick={() => steamTest.mutate()}
            >
              {steamTest.isPending ? 'Testing…' : 'Test API key'}
            </button>
            {!s.steam_api_key_set && <span className="text-sm text-dim">Paste a key below to test it.</span>}
            {test && 'ok' in test && (
              <span className="text-sm text-good">
                Works · {test.ok.persona_name ?? 'signed in'} · {plural(test.ok.game_count, 'game')}
              </span>
            )}
            {test && 'error' in test && (
              <span className="text-sm text-bad">
                {test.error}
                {s.steam_api_key_set && (
                  <>
                    . If the key is right, your profile's game details may be private:{' '}
                    <Ext href="https://steamcommunity.com/my/edit/settings">Steam privacy settings</Ext>.
                  </>
                )}
              </span>
            )}
          </div>
          <InlineField
            label={<span className="w-16 shrink-0">API key</span>}
            secret
            value=""
            placeholder={s.steam_api_key_set ? 'Saved. Paste a new one to replace it' : 'Paste your Steam Web API key'}
            className="font-mono text-xs"
            onCommit={(v) => save({ steam_api_key: v }, 'Steam API key', { inline: true })}
          />
          <InlineField
            label={<span className="w-16 shrink-0">SteamID</span>}
            value={s.steam_id ?? ''}
            placeholder="17-digit SteamID64"
            inputMode="numeric"
            className="font-mono text-xs"
            onCommit={(v) => save({ steam_id: v }, v ? 'SteamID' : 'SteamID cleared, back to auto-detect', { inline: true })}
          />
          <p className="m-0 text-[13px] leading-relaxed text-dim">
            The key is free at <Ext href="https://steamcommunity.com/dev/apikey">steamcommunity.com/dev/apikey</Ext>; use{' '}
            <span className="font-mono text-[11.5px]">localhost</span> as the domain. Your Steam game details need to be public.
            Clear the SteamID to go back to auto-detect.
          </p>
        </div>
      )}
    </Row>
  )
}

/** Connect opens a native GOG login window on this computer (GOG only redirects
 *  to its own page, so a browser tab can't hand the code back). Pasting the
 *  address it ends on is the fallback, and the only way when `noWindow`
 *  (no desktop, or another device; the setup wizard knows). */
/** GOG's sign-in: its window on the server's desktop, or, where none can
 *  open (`noWindow`: Docker, another device), the "Send to UpNext" bookmark,
 *  which comes back to `back`; pasting the address is the
 *  way round for both. */
export function GogRow({ noWindow: noWindowHere = false, back = '/settings' }: { noWindow?: boolean; back?: string }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [manual, setManual] = useState(false)
  // The window was refused (no desktop where UpNext runs): the bookmark from then on.
  const [refused, setRefused] = useState(false)
  const noWindow = noWindowHere || refused
  // Where no window can open: the paste form rather than the bookmark.
  const [paste, setPaste] = useState(false)
  const [code, setCode] = useState('')
  const gog = useGog({
    refetchInterval: (q) => (q.state.data?.login_pending ? 1500 : false),
  })
  const connected = gog.data?.connected === true
  const pending = gog.data?.login_pending === true

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: keys.profile })
    qc.invalidateQueries({ queryKey: keys.settings })
    qc.invalidateQueries({ queryKey: keys.config })
  }

  // Report how the login window ended (once per pending -> done transition).
  const wasPending = useRef(false)
  useEffect(() => {
    if (pending) {
      wasPending.current = true
      return
    }
    if (!wasPending.current || !gog.data) return
    wasPending.current = false
    if (gog.data.connected) {
      invalidate()
      toast.success('GOG connected', gog.data.username ? `Signed in as ${gog.data.username}` : undefined)
    } else if (gog.data.login_error) {
      toast.error("GOG login didn't finish", gog.data.login_error)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, gog.data])

  const login = useMutation({
    mutationFn: (c: string) => api.gogLogin(c),
    onSuccess: (st) => {
      qc.setQueryData(keys.gog, st)
      invalidate()
      setCode('')
      setManual(false)
      toast.success('GOG connected', st.username ? `Signed in as ${st.username}` : undefined)
    },
  })
  const openWindow = useMutation({
    mutationFn: api.gogWindow,
    onSuccess: (st) => qc.setQueryData(keys.gog, st),
    onError: (e) => {
      toast.error("Couldn't open the GOG login", errText(e))
      if (e instanceof ApiError && e.status === 409) setRefused(true)
      setManual(true)
    },
  })
  const logout = useMutation({
    mutationFn: api.gogLogout,
    onSuccess: (st) => {
      qc.setQueryData(keys.gog, st)
      invalidate()
      toast.info('GOG disconnected')
    },
    onError: (e) => toast.error("Couldn't disconnect", errText(e)),
  })

  let right
  if (gog.isLoading) right = <Status tone="dim">Checking…</Status>
  else if (gog.isError) right = <Status tone="dim">Status unavailable</Status>
  else if (connected)
    right = (
      <>
        <Status tone="good">{gog.data?.username ?? 'Connected'}</Status>
        <TextButton disabled={logout.isPending} onClick={() => logout.mutate()}>
          Disconnect
        </TextButton>
      </>
    )
  else
    right = (
      <>
        {pending ? (
          <Status tone="dim">
            <span className="animate-pulse">Waiting for the GOG window…</span>
          </Status>
        ) : (
          <Status tone={gog.data?.login_error ? 'bad' : 'dim'} title={gog.data?.login_error ?? undefined}>
            {gog.data?.login_error ? "Login didn't finish" : 'Not connected'}
          </Status>
        )}
        {pending || manual || noWindow ? (
          <TextButton aria-expanded={manual} onClick={() => setManual((m) => !m)}>
            {manual ? 'Hide' : noWindow ? 'Connect' : 'Paste instead'}
          </TextButton>
        ) : (
          <TextButton disabled={openWindow.isPending} onClick={() => openWindow.mutate()}>
            Connect
          </TextButton>
        )}
      </>
    )

  return (
    <Row name="GOG" desc={pending ? 'Finish signing in in the window that opened on this computer' : 'Playtime from GOG and Galaxy'} right={right}>
      {!connected && !pending && !manual && !noWindow && !gog.isLoading && (
        <p className="m-0 pb-1 text-[13px] leading-relaxed text-dim">
          Connect opens GOG’s sign-in in a window of its own on this computer, which may open behind this one. Google doesn’t allow
          signing in inside an app’s window: if you sign in to GOG with Google, or another account’s sign-in doesn’t finish there,{' '}
          <button
            type="button"
            className="text-muted underline decoration-underline underline-offset-4 hover:text-ink hover:decoration-accent"
            onClick={() => setManual(true)}
          >
            paste instead
          </button>
          .
        </p>
      )}
      {!connected && manual && noWindow && !paste && <GogBookmark back={back} onPaste={() => setPaste(true)} />}
      {!connected && (manual || pending) && (!noWindow || paste) && (
        <div className="space-y-2 pb-1">
          <p className="m-0 text-[13px] leading-relaxed text-dim">
            {pending ? 'No window? ' : ''}Sign in on GOG's own{' '}
            {gog.data?.auth_url ? <Ext href={gog.data.auth_url}>login page</Ext> : 'login page'}. It ends on a blank page: paste
            that page's address here within a few minutes.
          </p>
          <form
            className="flex flex-wrap items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault()
              if (code.trim()) login.mutate(code.trim())
            }}
          >
            <input
              className={cx('field min-w-0 flex-1 font-mono text-xs', login.isError && 'border-bad')}
              placeholder="Paste the address or code"
              aria-label="GOG login address or code"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <button type="submit" className="btn-outline" disabled={!code.trim() || login.isPending}>
              {login.isPending ? 'Connecting…' : 'Connect'}
            </button>
          </form>
          {login.isError && (
            <p role="alert" className="m-0 text-[12.5px] text-bad">
              {errText(login.error)}
            </p>
          )}
          {!pending && !noWindow && (
            <TextButton disabled={openWindow.isPending} onClick={() => openWindow.mutate()}>
              {openWindow.isIdle && !gog.data?.login_error ? 'Use the login window instead' : 'Try the login window again'}
            </TextButton>
          )}
        </div>
      )}
    </Row>
  )
}

/** `setup`: the wizard's, which it already holds. Settings holds none, so
 *  the row asks for it there, and only at the computer UpNext runs on, the
 *  one place the server lists other users' folders. */
export function HeroicRow({ s, setup }: { s: Settings; setup?: SetupInfo }) {
  const own = useQuery({
    queryKey: ['setup'],
    queryFn: api.setup,
    staleTime: 30_000,
    enabled: !setup && s.listen.this_computer && !s.listen.container,
  })
  const info = setup ?? own.data
  return (
    <Row
      name="Heroic"
      desc={
        s.heroic_dir ? (
          <span title={s.heroic_dir}>
            Epic, GOG and Amazon installs · <span className="font-mono text-[11.5px]">{s.heroic_dir}</span>
          </span>
        ) : (
          'Epic, GOG and Amazon installs'
        )
      }
      right={s.heroic_dir ? <Status tone="good">Found automatically</Status> : <Status tone="dim">Not found</Status>}
    >
      {info && <OtherUsersFolders setup={info} />}
    </Row>
  )
}

export function RawgRow({ s, save }: { s: Settings; save: SaveFn }) {
  const [open, setOpen] = useState(false)
  const { multi } = usePlayers()
  return (
    <Row
      name="RAWG"
      tag={everyones(multi, 'optional')}
      desc="Tags and ratings Steam lacks"
      right={
        <>
          {s.rawg_api_key_set ? <Status tone="good">Key saved</Status> : <Status tone="dim">No key</Status>}
          <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            {open ? 'Done' : s.rawg_api_key_set ? 'Replace' : 'Add key'}
          </TextButton>
        </>
      }
    >
      {open && (
        <div className="space-y-2 pb-1">
          <InlineField
            label={<span className="w-16 shrink-0">API key</span>}
            secret
            value=""
            placeholder={s.rawg_api_key_set ? 'Saved. Paste a new one to replace it' : 'Paste your RAWG key'}
            className="font-mono text-xs"
            onCommit={(v) => save({ rawg_api_key: v }, 'RAWG key', { inline: true })}
          />
          <p className="m-0 text-[13px] leading-relaxed text-dim">
            Free at <Ext href="https://rawg.io/apidocs">rawg.io/apidocs</Ext>. It asks for your site's address:{' '}
            <span className="font-mono text-[11.5px]">https://localhost</span> is fine. The free tier covers 20,000 lookups a
            month and a full pass uses about 2,000.
          </p>
        </div>
      )}
    </Row>
  )
}

export function IgdbRow({ s, save }: { s: Settings; save: SaveFn }) {
  const [open, setOpen] = useState(false)
  const ready = !!s.igdb_client_id && s.igdb_secret_set
  const half = !ready && (!!s.igdb_client_id || s.igdb_secret_set)
  const { multi } = usePlayers()
  return (
    <Row
      name="IGDB"
      tag={everyones(multi, 'optional')}
      desc="Extra keywords and trailer links"
      right={
        <>
          {ready ? (
            <Status tone="good">Connected</Status>
          ) : half ? (
            <Status tone="warn">{s.igdb_client_id ? 'Needs the secret' : 'Needs the client ID'}</Status>
          ) : (
            <Status tone="dim">Not set up</Status>
          )}
          <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            {open ? 'Done' : ready || half ? 'Change' : 'Set up'}
          </TextButton>
        </>
      }
    >
      {open && (
        <div className="space-y-2 pb-1">
          <p className="m-0 text-[13px] leading-relaxed text-dim">
            Fiddly to set up and fine to skip. Make a Twitch app at{' '}
            <Ext href="https://dev.twitch.tv/console/apps">dev.twitch.tv/console/apps</Ext> (Twitch wants two-factor on first):
            redirect URL <span className="font-mono text-[11.5px]">https://localhost</span>, category Application Integration,
            client type Confidential. Then copy the client ID and make a new secret.
          </p>
          <InlineField
            label={<span className="w-16 shrink-0">Client ID</span>}
            value={s.igdb_client_id ?? ''}
            placeholder="Twitch app client ID"
            className="font-mono text-xs"
            onCommit={(v) => save({ igdb_client_id: v }, 'IGDB client ID', { inline: true })}
          />
          <InlineField
            label={<span className="w-16 shrink-0">Secret</span>}
            secret
            value=""
            placeholder={s.igdb_secret_set ? 'Saved. Paste a new one to replace it' : 'Paste the client secret'}
            className="font-mono text-xs"
            onCommit={(v) => save({ igdb_client_secret: v }, 'IGDB secret', { inline: true })}
          />
        </div>
      )}
    </Row>
  )
}

/** GOG Galaxy's database: its path on this computer, or a copy uploaded
 *  from a Windows PC (`POST /api/galaxy/upload`), kept beside UpNext's own. */
export function GalaxyRow({ s, save }: { s: Settings; save: SaveFn }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [open, setOpen] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)
  // On Windows Galaxy is on this PC, and UpNext finds its database on its own.
  const windows = s.os === 'windows'
  // A path names a file on the computer UpNext runs on, so only it can set one.
  const here = s.listen.this_computer
  const upload = useMutation({
    mutationFn: api.galaxyUpload,
    onSuccess: (r) => {
      qc.invalidateQueries({ queryKey: keys.settings })
      qc.invalidateQueries({ queryKey: ['setup'] })
      toast.success('Galaxy database taken in', `${plural(r.games, 'game')} in it; the next sync reads ${r.games === 1 ? 'it' : 'them'}`)
    },
    onError: (e) => toast.error("Couldn't take that file", errText(e)),
  })
  return (
    <Row
      name="GOG Galaxy database"
      tag="optional"
      desc={
        s.galaxy_db ? (
          <span className="font-mono text-[11.5px]" title={s.galaxy_db}>
            {s.galaxy_db}
          </span>
        ) : windows ? (
          "GOG Galaxy isn't installed on this PC"
        ) : (
          'A galaxy-2.0.db copy from Windows'
        )
      }
      right={
        <>
          {!s.galaxy_db ? (
            // On Windows the path is found by itself when Galaxy is installed.
            <Status tone="dim">{windows ? 'Not installed' : 'Not set up'}</Status>
          ) : s.galaxy_db_exists ? (
            <Status tone="good">Found</Status>
          ) : (
            <Status tone="warn">File not found</Status>
          )}
          <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            {open ? 'Done' : s.galaxy_db ? 'Change' : windows ? 'Set path' : 'Add'}
          </TextButton>
        </>
      }
    >
      {open && (
        <div className="space-y-2 pb-1">
          {here && (
            <InlineField
              label={<span className="w-16 shrink-0">Path</span>}
              value={s.galaxy_db ?? ''}
              placeholder={windows ? 'C:\\ProgramData\\GOG.com\\Galaxy\\storage\\galaxy-2.0.db' : '/path/to/galaxy-2.0.db'}
              className="font-mono text-xs"
              onCommit={(v) => save({ galaxy_db: v }, v ? 'Galaxy database' : 'Galaxy database removed', { inline: true })}
            />
          )}
          {!windows && (
            <div className="flex flex-wrap items-center gap-3">
              <input
                ref={fileInput}
                type="file"
                accept=".db,.sqlite,application/vnd.sqlite3,application/x-sqlite3"
                className="sr-only"
                tabIndex={-1}
                aria-hidden
                onChange={(e) => {
                  const f = e.target.files?.[0]
                  if (f) upload.mutate(f)
                  e.target.value = ''
                }}
              />
              <button type="button" className="btn-outline" disabled={upload.isPending} onClick={() => fileInput.current?.click()}>
                {upload.isPending ? 'Uploading…' : 'Upload a copy…'}
              </button>
              <span className="text-[13px] text-dim">UpNext keeps it beside its own database and reads it on each sync.</span>
            </div>
          )}
          {windows ? (
            <p className="m-0 text-[13px] leading-relaxed text-dim">
              UpNext reads it from <span className="font-mono text-[11.5px]">C:\ProgramData\GOG.com\Galaxy\storage</span>, where
              Galaxy keeps it, and reads a copy, so Galaxy can stay open. Set a path only if yours is somewhere else.
            </p>
          ) : (
            <p className="m-0 text-[13px] leading-relaxed text-dim">
              On Windows it lives in <span className="font-mono text-[11.5px]">C:\ProgramData\GOG.com\Galaxy\storage</span>. Upload a
              copy{here ? ', or copy it somewhere this computer can read and give its path. Clear the path to stop using it' : ''}.
            </p>
          )}
        </div>
      )}
    </Row>
  )
}
