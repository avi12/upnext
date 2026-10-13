// The setup wizard's steps and words.

import type { SetupBackgroundAsk, SetupInfo } from '../types'

/** The wizard's steps, in order (`STEPS` in src/server/setup.rs). */
export const STEPS = [
  { id: 'welcome', label: 'Welcome' },
  { id: 'where', label: 'Where UpNext runs' },
  { id: 'steam', label: 'Steam' },
  { id: 'stores', label: 'GOG, Epic and Amazon' },
  { id: 'more_stores', label: 'Epic, EA and Ubisoft' },
  { id: 'extras', label: 'Extras' },
  { id: 'ai', label: 'AI' },
  { id: 'ai_jobs', label: 'What the AI does' },
  { id: 'sync', label: 'First sync' },
  { id: 'details', label: 'Details' },
  { id: 'taste', label: 'Your taste' },
  { id: 'players', label: 'Who plays here' },
  { id: 'done', label: 'Done' },
] as const

export type StepId = (typeof STEPS)[number]['id']

/** A new player's own short setup, in order (`PLAYER_STEPS` in
 *  src/server/setup.rs): the first time someone else is chosen, after the
 *  household's wizard. */
export const PLAYER_STEPS = [
  { id: 'you', label: 'You' },
  { id: 'games', label: 'Your games' },
  { id: 'ai', label: 'Your AI' },
  { id: 'taste', label: 'Your taste' },
  { id: 'done', label: 'Done' },
] as const

export type PlayerStepId = (typeof PLAYER_STEPS)[number]['id']

const at = (id: StepId) => STEPS.findIndex((x) => x.id === id)

/** What moving from one step to another starts in the background:
 *  the sync once the stores are behind (Epic's sign-in is on the
 *  second store step), so it runs while the person goes
 *  on, and Fetch details once the data-source keys are too. Until then
 *  details waits, so it doesn't start after the sync without the keys.
 *  The server syncs again only when an account changed. Null when nothing
 *  is crossed (going back starts nothing), and from the welcome step, whose
 *  jump ahead follows a database import that brought its own library. */
export function backgroundAsk(from: StepId, to: StepId): SetupBackgroundAsk | null {
  if (from === 'welcome') return null
  const crossed = (step: StepId) => at(from) <= at(step) && at(to) > at(step)
  if (!crossed('more_stores') && !crossed('extras')) return null
  return { ...(crossed('more_stores') ? { sync: 'if_changed' } : {}), details: crossed('extras') ? 'after_sync' : 'wait' }
}

export function osName(os: string) {
  return os === 'windows' ? 'Windows' : os === 'macos' ? 'macOS' : os === 'linux' ? 'Linux' : os
}

type Env = SetupInfo['environment']

/** Where UpNext runs and where the person is, in one sentence. */
export function whereText(env: Env, app: boolean) {
  const os = osName(env.os)
  if (env.container) return 'UpNext is in a Docker container, so it can only see the folders you mounted into it.'
  if (env.flatpak) {
    return app
      ? `You're running the UpNext app on ${os}, as a Flatpak.`
      : `UpNext runs as a Flatpak on ${env.client_local ? `this ${os} computer, and you're in a browser on it` : `a ${os} computer, and you're on another device`}.`
  }
  if (app) {
    if (env.mode === 'app') return `You're running the UpNext app on ${os}.`
    if (env.mode === 'service') return `You're in the UpNext app, showing the UpNext service on this ${os} computer.`
    return `You're in the UpNext app, showing an UpNext started from a terminal on this ${os} computer.`
  }
  const where = env.client_local ? `this ${os} computer` : `a ${os} computer`
  const you = env.client_local ? "and you're in a browser on it" : "and you're on another device"
  if (env.mode === 'app') return `UpNext runs inside the UpNext app on ${where}, ${you}.`
  if (env.mode === 'service') return `UpNext runs as a service on ${where}, ${you}.`
  return `UpNext runs on ${where}, ${you}.`
}

/** How long UpNext keeps running, which is when its schedules and jobs run. */
export function runsWhileText(env: Env) {
  if (env.container) return null
  if (env.mode === 'app')
    return "It runs while the app's window is open. Closing it stops the schedules and any job, and a job carries on where it stopped the next time you open the app."
  if (env.mode === 'service') return 'The service keeps running, with its schedules, when you close the window or the browser.'
  return env.client_local ? 'It runs as long as the terminal it was started from stays open.' : null
}

/** A way to keep UpNext running in the background, where there is one. */
export function backgroundHint(env: Env) {
  return !env.container && !env.flatpak && env.mode !== 'service' && env.os === 'linux'
    ? 'To keep it running in the background, with the daily sync, install its service, as “Start it at login” in docs/running.md says.'
    : null
}

/** Why GOG's sign-in window can't open on the computer UpNext runs on, and
 *  how to get it, for a browser or app on that computer. Null when it can. */
export function noWindowText(env: Env): { why: string; commands?: string[] } | null {
  if (env.gog_window || env.container || !env.client_local) return null
  const instead = 'GOG connects through a “Send to UpNext” bookmark instead of a sign-in window'
  if (env.display) return { why: `This UpNext was built without its windows, so ${instead}.` }
  if (env.os === 'linux' && env.mode === 'service')
    return {
      why: `The service started without your desktop, so ${instead}. To have the window, run these in a terminal on your desktop:`,
      commands: ['systemctl --user import-environment DISPLAY XAUTHORITY', 'systemctl --user restart upnext'],
    }
  if (env.os === 'windows') return { why: `UpNext runs outside your Windows sign-in, as a service, so it can't open windows on your desktop and ${instead}.` }
  return { why: `This computer has no desktop for UpNext's windows, so ${instead}.` }
}

/** In a Flatpak (`id`): what its sandbox reads, and the commands that let it
 *  read a library kept somewhere else and point UpNext at it, which hold for
 *  every start (`flatpak override`). */
export function flatpakFolders(id: string) {
  return {
    text: `UpNext is a Flatpak here. Its sandbox reads Steam's and Heroic's usual folders (and those of their Flatpaks) and nothing else of yours, and keeps UpNext's own files in ~/.var/app/${id}. For a library somewhere else, this lets it read the folder and points UpNext at it; then start UpNext again:`,
    commands: [
      `flatpak override --user --filesystem=/path/to/Steam:ro --env=UPNEXT_STEAM_DIR=/path/to/Steam ${id}`,
      `flatpak override --user --filesystem=/path/to/heroic:ro --env=UPNEXT_HEROIC_DIR=/path/to/heroic ${id}`,
    ],
  }
}

/** How to install Claude Code in a Flatpak (`id`): into the app's own folder,
 *  from a terminal, since the sandbox can't run the one on the computer. */
export function flatpakClaude(id: string) {
  return {
    install: `flatpak run --command=upnext ${id} install-claude`,
    token: `flatpak override --user --env=CLAUDE_CODE_OAUTH_TOKEN=<the token> ${id}`,
  }
}

/** How to install Claude Code on the computer UpNext runs on. */
export function claudeInstall(os: string) {
  return os === 'windows'
    ? { command: 'irm https://claude.ai/install.ps1 | iex', shell: 'PowerShell' }
    : { command: 'curl -fsSL https://claude.ai/install.sh | bash', shell: 'a terminal' }
}

/** Where GOG Galaxy keeps its database on Windows. */
export const GALAXY_WINDOWS_DIR = 'C:\\ProgramData\\GOG.com\\Galaxy\\storage'

/** The computer Docker runs on, for the mount lines. */
export type DockerHost = 'linux' | 'macos' | 'windows'

/** Where Steam and Heroic keep their files on each kind of host; `none`
 *  leaves the library out. Linux has the Flatpak and Snap installs too. */
export const MOUNT_CHOICES = {
  steam: {
    linux: [
      ['native', '~/.local/share/Steam'],
      ['flatpak', '~/.var/app/com.valvesoftware.Steam/.local/share/Steam'],
      ['snap', '~/snap/steam/common/.local/share/Steam'],
    ],
    macos: [['native', '~/Library/Application Support/Steam']],
    windows: [['native', 'C:\\Program Files (x86)\\Steam']],
  },
  heroic: {
    linux: [
      ['native', '~/.config/heroic'],
      ['flatpak', '~/.var/app/com.heroicgameslauncher.hgl/config/heroic'],
    ],
    macos: [['native', '~/Library/Application Support/heroic']],
    windows: [['native', '%APPDATA%\\heroic']],
  },
} as const satisfies Record<'steam' | 'heroic', Record<DockerHost, readonly (readonly [string, string])[]>>

export interface MountChoice {
  host: DockerHost
  /** An install from `MOUNT_CHOICES`, or `none`. */
  steam: string
  heroic: string
  /** The time zone the daily sync's hour follows: the browser's own. */
  tz: string
}

/** The folder for a library's choice, as the host's own shell would name it, or null. */
function folder(c: MountChoice, store: 'steam' | 'heroic') {
  const found = (MOUNT_CHOICES[store][c.host] as readonly (readonly [string, string])[]).find(([id]) => id === c[store])
  return found ? found[1] : null
}

/** A path for compose's file: `~` and `${APPDATA}` are expanded by compose
 *  itself, and Windows paths take forward slashes. */
function composePath(p: string) {
  return p.replace('%APPDATA%', '${APPDATA}').replace(/\\/g, '/')
}

/** A path for `docker run` in the host's shell: `$HOME` (in quotes, for
 *  macOS's spaces) on Linux and macOS, PowerShell's `$env:APPDATA` on Windows. */
function shellPath(p: string) {
  return p.replace(/^~/, '$HOME').replace('%APPDATA%', '$env:APPDATA')
}

const IMAGE = 'ghcr.io/zytocc/upnext:latest'

/** The lines that mount the chosen libraries into UpNext's container,
 *  read-only, as contrib/docker/compose.yaml has them commented out: for
 *  compose (its `volumes:` and `environment:` entries), and a whole
 *  `docker run` for the host's shell. */
export function mountLines(c: MountChoice) {
  const mounts = (['steam', 'heroic'] as const).flatMap((store) => {
    const from = folder(c, store)
    return from ? [{ from, to: `/host/${store}`, env: `UPNEXT_${store.toUpperCase()}_DIR` }] : []
  })
  const compose = [
    '    volumes:',
    '      - upnext-data:/data',
    ...mounts.map((m) => `      - "${composePath(m.from)}:${m.to}:ro"`),
    '    environment:',
    `      TZ: ${c.tz}`,
    ...mounts.map((m) => `      ${m.env}: ${m.to}`),
  ].join('\n')
  const windows = c.host === 'windows'
  const next = windows ? ' `' : ' \\'
  const run = [
    'docker run -d --name upnext --restart unless-stopped',
    '  -p 127.0.0.1:8787:8787',
    `  -v upnext-data:/data -e TZ=${c.tz}`,
    ...mounts.map((m) => `  -v "${shellPath(m.from)}:${m.to}:ro" -e ${m.env}=${m.to}`),
    '  --add-host host.docker.internal:host-gateway',
    '  --stop-timeout 30',
    '  --read-only --tmpfs /tmp --security-opt no-new-privileges --cap-drop ALL',
    `  ${IMAGE}`,
  ]
    .map((line, i, all) => (i < all.length - 1 ? line + next : line))
    .join('\n')
  return { compose, run, mounted: mounts.length }
}
