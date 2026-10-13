import { useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api, ApiError } from '../../api'
import { canSwitch } from '../../lib/connect'
import { plural, storeName } from '../../lib/format'
import { progressText } from '../../lib/jobs'
import { GalaxyRow, GogRow, HeroicRow, IgdbRow, RawgRow, SteamRow } from '../settings/Accounts'
import { steamIdText } from '../settings/accountText'
import { LinuxRow } from '../settings/Looks'
import { Ext, RowGrid, Segmented } from '../settings/parts'
import { ProgressBar } from '../ProgressBar'
import { useToast } from '../useToast'
import {
  backgroundHint,
  type DockerHost,
  flatpakFolders,
  GALAXY_WINDOWS_DIR,
  MOUNT_CHOICES,
  mountLines,
  noWindowText,
  runsWhileText,
  whereText,
} from '../../lib/setup'
import { inApp } from '../../lib/frame'
import { CopyLines, Found, StepButtons, StepHead, type StepProps } from './parts'
import { OtherUsersFolders } from './OtherUsersFolders'
import { useJobs } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** 1. What UpNext does and what setup involves; or bring a database over. */
export function WelcomeStep({ setup, go, next }: StepProps) {
  const qc = useQueryClient()
  const toast = useToast()
  const fileInput = useRef<HTMLInputElement>(null)
  const [importing, setImporting] = useState<number | null>(null)
  const env = setup.environment
  const jobs = useJobs({
    enabled: importing != null,
    refetchInterval: importing != null ? 1000 : false,
  })
  const importDb = useMutation({
    mutationFn: api.importDatabase,
    onSuccess: (j) => setImporting(j.id),
    onError: (e) => toast.error("Couldn't import that", errText(e)),
  })
  const job = importing != null ? (jobs.data?.current?.id === importing ? jobs.data.current : jobs.data?.recent.find((j) => j.id === importing)) : null
  const finished = job && job.status !== 'running'

  // Once the import is in: straight to Done, or to the AI step when the file had no AI set up.
  const after = async () => {
    setImporting(null)
    await qc.invalidateQueries()
    const fresh = await qc.fetchQuery({ queryKey: ['setup'], queryFn: api.setup })
    go(fresh.ai.available ? 'done' : 'ai')
  }

  return (
    <section aria-labelledby="step-welcome" className="flex flex-col gap-6">
      <StepHead id="step-welcome" title="Welcome to UpNext">
        <p className="m-0">
          UpNext reads the games you own on Steam, GOG, Epic and Amazon, learns what you play, and picks what to play next, with
          the reasons why.
        </p>
        <p className="m-0">
          Setup connects your stores, adds a few optional keys and chooses an AI: about five minutes. Fetching each game's
          details then runs in the background while you look around. Every step can be skipped and done later in Settings.
        </p>
        <p className="m-0">
          Everything stays {env.container ? 'with this server' : 'on this computer'}: keys live in UpNext's own database, and
          nothing goes to an account of ours, because there isn't one.
        </p>
      </StepHead>

      {env.client_local && (
        <div className="flex flex-col gap-2 border-t border-line pt-5">
          <div className="text-[15.5px] font-bold">Moving from another computer?</div>
          <p className="m-0 max-w-[720px] text-[14px] text-dim">
            Import your UpNext database (a backup, or Settings › Backups on the old computer) and skip the rest. A copy without
            keys brings everything but them; you'll choose an AI again.
          </p>
          <input
            ref={fileInput}
            type="file"
            accept=".db,.sqlite,.sqlite3,application/vnd.sqlite3,application/x-sqlite3"
            className="sr-only"
            tabIndex={-1}
            aria-hidden
            onChange={(e) => {
              const f = e.target.files?.[0]
              if (f) importDb.mutate(f)
              e.target.value = ''
            }}
          />
          {job ? (
            <div aria-live="polite" className="max-w-[560px]">
              {!finished ? (
                <>
                  <span className="text-[14px] text-muted">Importing… {progressText(job)}</span>
                  <ProgressBar job={job} label="Import progress" />
                </>
              ) : job.status === 'done' ? (
                <div className="flex flex-wrap items-center gap-3">
                  <span className="text-[14px] text-good">Imported{job.summary ? `: ${job.summary}` : ''}.</span>
                  <button type="button" className="btn-outline" onClick={() => void after()}>
                    Continue
                  </button>
                </div>
              ) : (
                <span className="text-[14px] text-bad">The import didn't finish: {job.error ?? job.status}</span>
              )}
            </div>
          ) : (
            <div>
              <button type="button" className="btn-outline" disabled={importDb.isPending} onClick={() => fileInput.current?.click()}>
                {importDb.isPending ? 'Uploading…' : 'Import a database…'}
              </button>
            </div>
          )}
        </div>
      )}
      {env.client_local && canSwitch() && (
        <div className="flex flex-col gap-2 border-t border-line pt-5">
          <div className="text-[15.5px] font-bold">Is your library on another computer?</div>
          <p className="m-0 max-w-[720px] text-[14px] text-dim">
            A handheld or a second PC can use the UpNext on your main computer instead of setting up its own. Games still start here.
          </p>
          <div>
            <Link to="/connect" className="btn-outline">
              Use the library on another computer
            </Link>
          </div>
        </div>
      )}
      <StepButtons onNext={next} next="Start" />
    </section>
  )
}

/** 2. Shown, not asked: where UpNext runs and what it can reach from there. */
export function WhereStep({ setup, next, back }: StepProps) {
  const env = setup.environment
  const { steam, heroic, galaxy } = setup.stores
  const intro = whereText(env, inApp())
  const runsWhile = runsWhileText(env)
  const hint = backgroundHint(env)
  const noWindow = noWindowText(env)
  const heroicGames = Object.entries(heroic.games)
    .map(([store, n]) => `${plural(n, `${storeName(store)} game`)}`)
    .join(', ')
  return (
    <section aria-labelledby="step-where" className="flex flex-col gap-6">
      <StepHead id="step-where" title="Where UpNext runs">
        <p className="m-0 text-ink">{intro}</p>
        {runsWhile && (
          <p className="m-0">
            {runsWhile}
            {hint ? ` ${hint}` : ''}
          </p>
        )}
        {!env.client_local && !env.container && (
          <p className="m-0">
            Setup works from here through your pairing. Two things only work on the computer UpNext runs on: GOG's sign-in window,
            and giving the path of a file there. Both have a way round in their steps.
          </p>
        )}
        {noWindow && (
          <>
            <p className="m-0">{noWindow.why}</p>
            {noWindow.commands && (
              <pre className="m-0 font-mono text-[13px] whitespace-pre-wrap text-ink [overflow-wrap:anywhere]">{noWindow.commands.join('\n')}</pre>
            )}
          </>
        )}
        {env.flatpak && <FlatpakFolders id={env.flatpak} />}
        {env.container && (
          <p className="m-0">
            Mount the Steam and Heroic folders read-only to have their games found: choose yours below and copy the lines. Without
            them, Steam still works through its Web API key, GOG through its sign-in, and GOG Galaxy through an uploaded copy.
          </p>
        )}
      </StepHead>
      <ul className="m-0 flex list-none flex-col gap-2.5 p-0">
        <Found ok={steam.path ? true : null}>
          {steam.path ? (
            <>
              Steam at <span className="font-mono text-[13px] break-words">{steam.path}</span>
              {steam.local_games != null && `, ${plural(steam.local_games, 'game')} in its files`}
              {steam.persona && `, signed in as ${steam.persona}`}
            </>
          ) : (
            `Steam's folder: ${steam.missing ?? 'not found'}`
          )}
        </Found>
        <Found ok={heroic.path ? true : null}>
          {heroic.path ? (
            <>
              Heroic at <span className="font-mono text-[13px] break-words">{heroic.path}</span>
              {heroicGames ? `: ${heroicGames}` : ', with no store signed in yet'}
            </>
          ) : (
            `Heroic (Epic, GOG and Amazon): ${heroic.missing ?? 'not found'}`
          )}
        </Found>
        <Found ok={galaxy.exists ? true : null}>
          {galaxy.exists ? (
            <>
              GOG Galaxy's database at <span className="font-mono text-[13px] break-words">{galaxy.path}</span>
            </>
          ) : env.os === 'windows' ? (
            <>
              GOG Galaxy: not installed (its database would be in{' '}
              <span className="font-mono text-[13px] break-words">{GALAXY_WINDOWS_DIR}</span>)
            </>
          ) : (
            "GOG Galaxy's database: not here (it's on Windows; you can upload a copy)"
          )}
        </Found>
        {(steam.error || heroic.error) && (
          <li className="text-[13.5px] text-bad">Reading them went wrong: {steam.error ?? heroic.error}</li>
        )}
      </ul>
      <OtherUsersFolders setup={setup} heroic={false} steam />
      {env.container && <MountLines steamFound={!!steam.path} heroicFound={!!heroic.path} />}
      <StepButtons onNext={next} onBack={back} />
    </section>
  )
}

/** In a Flatpak: what the sandbox reads, and how to let it read more. */
function FlatpakFolders({ id }: { id: string }) {
  const { text, commands } = flatpakFolders(id)
  return (
    <>
      <p className="m-0">{text}</p>
      <CopyLines label="For a terminal" text={commands.join('\n')} />
    </>
  )
}

/** In a container: the lines that mount Steam's and Heroic's folders, for
 *  the host the person picks (the server can't see the host's folders). */
function MountLines({ steamFound, heroicFound }: { steamFound: boolean; heroicFound: boolean }) {
  const [host, setHost] = useState<DockerHost>(() => (/Windows/.test(navigator.userAgent) ? 'windows' : /Mac OS/.test(navigator.userAgent) ? 'macos' : 'linux'))
  const [steam, setSteam] = useState('native')
  const [heroic, setHeroic] = useState('native')
  const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
  // An install the new host doesn't have falls back to its first.
  const pick = (store: 'steam' | 'heroic', id: string) =>
    id === 'none' || MOUNT_CHOICES[store][host].some(([x]) => x === id) ? id : MOUNT_CHOICES[store][host][0][0]
  const choice = { host, steam: pick('steam', steam), heroic: pick('heroic', heroic), tz }
  const lines = mountLines(choice)
  const LABEL: Record<string, string> = { native: 'Installed', flatpak: 'Flatpak', snap: 'Snap', none: 'Not used' }
  const options = (store: 'steam' | 'heroic') => [
    ...MOUNT_CHOICES[store][host].map(([id]) => ({ value: id, label: LABEL[id] })),
    { value: 'none', label: LABEL.none },
  ]
  return (
    <div className="flex flex-col gap-4 border-t border-line pt-5">
      <div className="text-[15.5px] font-bold">Add your libraries</div>
      <div className="grid max-w-[720px] grid-cols-1 items-center gap-x-4 gap-y-3 text-[14px] text-muted sm:grid-cols-[150px_1fr]">
        <span>Docker runs on</span>
        <Segmented
          label="Docker runs on"
          value={host}
          onChange={setHost}
          options={[
            { value: 'linux', label: 'Linux' },
            { value: 'windows', label: 'Windows' },
            { value: 'macos', label: 'macOS' },
          ]}
        />
        <span>Steam{steamFound ? ' (mounted)' : ''}</span>
        <Segmented label="Steam" value={choice.steam} onChange={setSteam} options={options('steam')} />
        <span>Heroic{heroicFound ? ' (mounted)' : ''}</span>
        <Segmented label="Heroic" value={choice.heroic} onChange={setHeroic} options={options('heroic')} />
      </div>
      <CopyLines label="In your compose file (contrib/docker/compose.yaml), the service's volumes and environment" text={lines.compose} />
      <CopyLines label={`Or one docker run${host === 'windows' ? ', in PowerShell' : ''}, in place of compose`} text={lines.run} />
      <p className="m-0 max-w-[720px] text-[13.5px] leading-relaxed text-dim">
        Then <span className="font-mono text-[12.5px]">docker compose up -d</span> again (or remove the old container and run the
        line): the data volume keeps everything, this setup's place included. Steam or Heroic somewhere else? Change the folder
        before the first colon. If the files belong to a user other than uid 1000, add{' '}
        <span className="font-mono text-[12.5px]">user: "UID:GID"</span> (<span className="font-mono text-[12.5px]">--user</span>) with
        yours, from <span className="font-mono text-[12.5px]">id -u</span> and <span className="font-mono text-[12.5px]">id -g</span>.
      </p>
    </div>
  )
}

/** 3. Steam: the sign-in fills in the SteamID; the Web API key is the true list. */
export function SteamStep({ setup, s, save, next, back }: StepProps) {
  const steam = setup.stores.steam
  const app = inApp()
  return (
    <section aria-labelledby="step-steam" className="flex flex-col gap-6">
      <StepHead id="step-steam" title="Steam">
        <p className="m-0">
          {steam.path
            ? `UpNext found Steam on ${setup.environment.client_local ? 'this computer' : 'the server'}${steam.local_games != null ? ` with ${plural(steam.local_games, 'game')} in its files` : ''}, and can sync from those alone. `
            : ''}
          Signing in and adding a Web API key gives the true list of what you own, with playtime: the local files miss games
          you haven't installed and can list ones you no longer own.
        </p>
        <ol className="m-0 flex list-decimal flex-col gap-1 pl-5">
          <li>
            {s.steam_id ? (
              <>
                <b className="font-semibold text-ink">Done.</b> {steamIdText(s)}.
              </>
            ) : (
              <>
                <b className="font-semibold text-ink">Sign in with Steam</b> below. It fills in your SteamID.
                {app && ' Steam’s page opens in your browser; come back to this window once it says you’re signed in.'}
              </>
            )}
          </li>
          <li>
            Open <Ext href="https://steamcommunity.com/dev/apikey">steamcommunity.com/dev/apikey</Ext> and enter{' '}
            <span className="font-mono text-[13px]">localhost</span> as the domain{app ? ' (it opens in your browser too)' : ''}.
          </li>
          <li>
            Paste the key and press <b className="font-semibold text-ink">Test API key</b>. Your profile's game details need to
            be public (<Ext href="https://steamcommunity.com/my/edit/settings">privacy settings</Ext>).
          </li>
        </ol>
      </StepHead>
      <RowGrid>
        <SteamRow s={s} save={save} inWizard />
      </RowGrid>
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}

/** 4. GOG, and Epic and Amazon through Heroic, and GOG Galaxy. */
export function StoresStep({ setup, s, save, next, back }: StepProps) {
  const env = setup.environment
  const heroic = setup.stores.heroic
  const counts = Object.entries(heroic.games)
  return (
    <section aria-labelledby="step-stores" className="flex flex-col gap-6">
      <StepHead id="step-stores" title="GOG, Epic and Amazon">
        <p className="m-0">
          Connecting GOG brings your GOG library with its playtime.{' '}
          {env.gog_window && env.client_local
            ? 'Connect opens GOG’s sign-in in a window of its own; the GOG row says what it takes.'
            : 'GOG’s sign-in can’t open a window here, so Connect gives you a “Send to UpNext” bookmark: sign in on GOG’s page, click the bookmark there, and you’re back here, connected.'}
        </p>
        <p className="m-0">
          Epic and Amazon games come through the Heroic launcher.{' '}
          {heroic.path ? (
            counts.length ? (
              <>Heroic has {counts.map(([store, n]) => plural(n, `${storeName(store)} game`)).join(', ')}.</>
            ) : (
              'Heroic is here, with no store signed in yet.'
            )
          ) : (
            <>
              Heroic wasn't found{env.container ? ' (its folder isn’t mounted)' : ''}. If you use Epic or Amazon, get it from{' '}
              <Ext href="https://heroicgameslauncher.com">heroicgameslauncher.com</Ext>.
            </>
          )}
          {env.os === 'windows'
            ? ' The Epic Games Launcher’s installs are read too, and the next step signs in to Epic for the rest of your Epic games.'
            : ' Without Heroic, the next step signs in to Epic itself.'}
        </p>
        <p className="m-0">
          {env.os !== 'windows'
            ? 'GOG Galaxy (on Windows) keeps every store you linked in it, Epic and Amazon among them, with playtime. Upload a copy of its database to add them.'
            : setup.stores.galaxy.exists
              ? 'GOG Galaxy keeps every store you linked in it, Epic and Amazon among them, with playtime; UpNext reads it where Galaxy keeps it, and Galaxy can stay open.'
              : 'GOG Galaxy isn’t installed here. It’s optional: with it, every store you link in Galaxy, Epic and Amazon among them, comes in with playtime.'}
        </p>
      </StepHead>
      <RowGrid>
        <GogRow noWindow={!env.gog_window || !env.client_local} back="/setup" />
        <HeroicRow s={s} setup={setup} />
        <GalaxyRow s={s} save={save} />
      </RowGrid>
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}

/** 5. Optional keys, and Linux compatibility where it applies. */
export function ExtrasStep({ setup, s, save, next, back }: StepProps) {
  return (
    <section aria-labelledby="step-extras" className="flex flex-col gap-6">
      <StepHead id="step-extras" title="Extras">
        <p className="m-0">
          Both optional. RAWG fills in tags and ratings Steam lacks, for GOG, Epic and Amazon games most of all: a free key in two
          minutes. IGDB adds keywords and trailers, but needs a Twitch app with two-factor sign-in; fine to skip.
        </p>
        {setup.environment.os === 'linux' && (
          <p className="m-0">Linux compatibility shows ProtonDB ratings, and picks favour games that run well on Linux.</p>
        )}
      </StepHead>
      <RowGrid>
        <RawgRow s={s} save={save} />
        <IgdbRow s={s} save={save} />
        {setup.environment.os === 'linux' && <LinuxRow s={s} save={save} />}
      </RowGrid>
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}
