import type {
  NextInSeries,
  Routine,
  RoutineBody,
  Routines,
  LlmModels,
  CodexStatus,
  OpenAiKeyStatus,
  Config,
  EnrichOptions,
  EnrichSource,
  GamesQuery,
  GamesResponse,
  GameView,
  GogBrowserLogin,
  GogBookmarkKey,
  GogStatus,
  Job,
  JobDetail,
  JobsQuery,
  SetupReset,
  SetupResetDone,
  SetupBackground,
  SetupBackgroundAsk,
  HistoryQuery,
  JobsResponse,
  ScheduleChange,
  ScheduledResponse,
  TaskId,
  WishlistResponse,
  WishState,
  OverrideStatus,
  StopReason,
  PickBatch,
  PickResult,
  FeedbackAction,
  MissReason,
  PickVerdict,
  TooOutcome,
  Profile,
  ReviewVocabulary,
  Settings,
  SettingsPatch,
  Source,
  SplitResult,
  Stats,
  SteamTestResult,
  SyncLibrary,
  SyncOptions,
  Collection,
  DecideResult,
  LlmTestResult,
  AiUsageSummary,
  PlanSummary,
  PricesView,
  LogLevel,
  Backup,
  BackupsResponse,
  SeedApplied,
  SeedManifest,
  SeedStatus,
  SecondChanceAnswer,
  SecondChancesResponse,
  GemsResponse,
  WayIn,
  TasteResponse,
  TasteRow,
  TasteSources,
  TasteSourceId,
  ReactionRow,
  StatementBody,
  StatementEdit,
  DiaryRow,
  GameRatingRow,
  DiaryState,
  TasteQuestions,
  NewToken,
  TokenScope,
  TokensResponse,
  Pairing,
  PairingOutcome,
  SteamShortcut,
  Paired,
  PairAsked,
  PairCollected,
  WaitingDevices,
  NetworkNearby,
  NetworkConnection,
  SetupInfo,
  SetupState,
  SetupChange,
  GalaxyUploaded,
  ByHandLists,
  EpicStatus,
  ByHandSaved,
  AiEstimates,
  PickMode,
  VerdictAnswers,
  CommunityStatus,
  GameCommunity,
  VerdictConsent,
  VerdictContract,
  VerdictForm,
  VerdictProblem,
  VerdictSent,
  VerdictsOverview,
  MyVerdict,
  AccountsResponse,
  StoreConnection,
  AiAccount,
  HiddenTitle,
  HideAnswer,
  HideScope,
  OidcStatus,
  Player,
  PlayersResponse,
  Sharing,
  StoreAccount,
} from './types'

export class ApiError extends Error {
  status: number
  /** What's wrong field by field, when the answer says (a refused backlog verdict). */
  problems: VerdictProblem[]
  constructor(status: number, message: string, problems: VerdictProblem[] = []) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.problems = problems
  }
}

/** How the server starts a 403 that means nobody has been chosen to play yet. */
const CHOOSE_PLAYER = "choose who's playing"

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response
  try {
    res = await fetch(path, {
      ...init,
      headers: {
        Accept: 'application/json',
        ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
        ...(init?.headers ?? {}),
      },
    })
  } catch (e) {
    throw new ApiError(0, e instanceof Error ? e.message : 'Network error')
  }
  if (!res.ok) {
    // Another device that isn't paired (or no longer is): the app asks for a code.
    if (res.status === 401 && !path.endsWith('/pair')) window.dispatchEvent(new Event('upnext:unpaired'))
    let message = `${res.status} ${res.statusText}`
    let problems: VerdictProblem[] = []
    try {
      const body = (await res.json()) as { error?: string; problems?: VerdictProblem[] }
      if (body && typeof body.error === 'string') message = body.error
      if (body && Array.isArray(body.problems)) problems = body.problems
    } catch {
      /* ignore */
    }
    // The default player has a PIN and nobody is chosen in this browser: ask who's playing.
    if (res.status === 403 && message.startsWith(CHOOSE_PLAYER)) window.dispatchEvent(new Event('upnext:choose-player'))
    throw new ApiError(res.status, message, problems)
  }
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}

export const api = {
  profile: () => request<Profile>('/api/v1/profile'),
  config: () => request<Config>('/api/v1/config'),
  stats: () => request<Stats>('/api/v1/stats'),

  games: (query: GamesQuery = {}) => {
    const params = new URLSearchParams()
    if (query.status && query.status.length) params.set('status', query.status.join(','))
    if (query.staple) params.set('staple', 'true')
    if (query.source && query.source.length) params.set('source', query.source.join(','))
    if (query.installed) params.set('installed', 'true')
    if (query.controller) params.set('controller', 'true')
    if (query.q) params.set('q', query.q)
    if (query.genre) params.set('genre', query.genre)
    if (query.tag) params.set('tag', query.tag)
    if (query.linux) params.set('linux', query.linux)
    if (query.rating) params.set('rating', query.rating)
    if (query.backlog) params.set('backlog', query.backlog)
    for (const f of query.facet ?? []) params.append('facet', f)
    for (const a of query.praised ?? []) params.append('praised', a)
    for (const a of query.not_criticized ?? []) params.append('not_criticized', a)
    if (query.sort) params.set('sort', query.sort)
    if (query.order) params.set('order', query.order)
    if (query.limit != null) params.set('limit', String(query.limit))
    if (query.offset != null) params.set('offset', String(query.offset))
    const qs = params.toString()
    return request<GamesResponse>(`/api/v1/games${qs ? `?${qs}` : ''}`)
  },

  game: (id: number) => request<GameView>(`/api/v1/games/${id}`),
  /** Fetch a game's player reviews now: "What players wrote" on a game whose summary came from shared data. */
  fetchReviews: (id: number) => request<GameView>(`/api/v1/games/${id}/reviews`, { method: 'POST' }),

  secondChances: () => request<SecondChancesResponse>('/api/v1/second-chances'),

  /** An answer, Undo (the mark from before comes back), new reasons (the reply box is their note), or a mix. */
  secondChance: (
    id: number,
    body: { answer?: SecondChanceAnswer; undo?: boolean; reasons?: { codes: string[]; note?: string | null } },
  ) => request<GameView>(`/api/v1/second-chances/${id}`, { method: 'POST', body: JSON.stringify(body) }),

  reviewTags: () => request<ReviewVocabulary>('/api/v1/review-tags'),

  /** The Hidden gems shelf; `all` adds the whole pool, ranked. */
  gems: (all = false) => request<GemsResponse>(`/api/v1/gems${all ? '?all=true' : ''}`),

  /** The recommended wishlist: suggestions and the wishlist. */
  wishlist: () => request<WishlistResponse>('/api/v1/wishlist'),
  /** Put a suggestion on the wishlist (`id`), or find a game on Steam's store by name and add it. */
  wishlistAdd: (body: { id: number } | { name: string }) =>
    request<WishlistResponse>('/api/v1/wishlist', { method: 'POST', body: JSON.stringify(body) }),
  wishlistSet: (id: number, state: Exclude<WishState, 'in_library'>) =>
    request<WishlistResponse>(`/api/v1/wishlist/${id}`, { method: 'PUT', body: JSON.stringify({ state }) }),
  wishlistForget: (id: number) => request<WishlistResponse>(`/api/v1/wishlist/${id}`, { method: 'DELETE' }),

  /** The genres and mechanics a game is a way into for you now. */
  newToYou: (id: number) => request<{ ways: WayIn[] }>(`/api/v1/new-to-you/${id}`),

  /** The taste profile. */
  taste: () => request<TasteResponse>('/api/v1/taste'),
  tasteState: (body: StatementBody) => request<TasteRow>('/api/v1/taste/statements', { method: 'POST', body: JSON.stringify(body) }),
  tasteEdit: (id: number, body: StatementEdit) =>
    request<TasteRow>(`/api/v1/taste/statements/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  tasteDelete: (id: number) => request<void>(`/api/v1/taste/statements/${id}`, { method: 'DELETE' }),
  tasteSources: () => request<TasteSources>('/api/v1/taste/sources'),
  tasteReactions: () => request<ReactionRow[]>('/api/v1/taste/sources/reactions'),
  tasteDeleteSource: (source: TasteSourceId) => request<{ deleted: number }>(`/api/v1/taste/sources/${source}`, { method: 'DELETE' }),
  tasteExportUrl: '/api/v1/taste/export',
  tasteQuestions: () => request<TasteQuestions>('/api/v1/taste/questions'),
  tasteDiary: (on: boolean) => request<{ diary: DiaryState }>('/api/v1/taste/diary', { method: 'PUT', body: JSON.stringify({ on }) }),
  tasteGameRatings: () => request<GameRatingRow[]>('/api/v1/taste/sources/ratings'),
  tasteDiaryRows: () => request<DiaryRow[]>('/api/v1/taste/sources/diary'),
  /** The Play button: a start time for the play diary, when it's on. Never fails loudly. */
  diaryLaunch: (title_id: number) =>
    request<{ recorded: boolean }>('/api/v1/taste/diary/launch', { method: 'POST', body: JSON.stringify({ title_id }), keepalive: true }).catch(() => null),

  nextInSeries: (id: number) => request<NextInSeries | null>(`/api/v1/games/${id}/next-in-series`),

  setOverride: (id: number, status: OverrideStatus | null, reasons?: { codes: string[]; note?: string }) =>
    request<GameView>(`/api/v1/games/${id}/override`, {
      method: 'PUT',
      body: JSON.stringify({ status, ...(reasons ? { reasons } : {}) }),
    }),

  /** Your own rating of a game, 1 to 5 stars; null clears it. */
  rateGame: (id: number, rating: number | null) =>
    request<GameView>(`/api/v1/games/${id}/rating`, { method: 'PUT', body: JSON.stringify({ rating }) }),

  /** Whether the game can be played endlessly, set on its sheet; null lets UpNext infer it. */
  setNoEnding: (id: number, noEnding: boolean | null) =>
    request<GameView>(`/api/v1/games/${id}/ending`, { method: 'PUT', body: JSON.stringify({ no_ending: noEnding }) }),

  stopReasons: () => request<StopReason[]>('/api/v1/stop-reasons'),

  /** With `withoutAi`, from UpNext's own ranking even with an AI set up; with no AI set up, picks always are.
   *  `routineId` picks for one of your routines, and `mode` 'random' rolls the dice: games off
   *  your radar drawn by chance, never with the AI; `input` says what it's played on: big screen sends
   *  `controller`. */
  pick: (
    prompt: string,
    count = 3,
    excludeIds: number[] = [],
    sources?: Source[],
    avoid: string[] = [],
    withoutAi = false,
    routineId: number | null = null,
    mode?: PickMode,
    input?: 'controller',
  ) =>
    request<PickResult>('/api/v1/picks', {
      method: 'POST',
      body: JSON.stringify({
        prompt,
        count,
        exclude_ids: excludeIds,
        ...(avoid.length ? { avoid } : {}),
        ...(sources && sources.length ? { sources } : {}),
        ...(withoutAi ? { without_ai: true } : {}),
        ...(routineId != null ? { routine_id: routineId } : {}),
        ...(mode ? { mode } : {}),
        ...(input ? { input } : {}),
      }),
    }),

  /** Your routines, with the one that fits now. */
  routines: () => request<Routines>('/api/v1/routines'),
  addRoutine: (body: RoutineBody) => request<Routine>('/api/v1/routines', { method: 'POST', body: JSON.stringify(body) }),
  saveRoutine: (id: number, body: RoutineBody) =>
    request<Routine>(`/api/v1/routines/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  deleteRoutine: (id: number) => request<void>(`/api/v1/routines/${id}`, { method: 'DELETE' }),

  history: (limit = 20, query: HistoryQuery = {}) => {
    const params = new URLSearchParams({ limit: String(limit) })
    if (query.offset) params.set('offset', String(query.offset))
    if (query.order && query.order !== 'newest') params.set('order', query.order)
    if (query.since != null) params.set('since', String(query.since))
    if (query.q?.trim()) params.set('q', query.q.trim())
    if (query.outcome) params.set('outcome', query.outcome)
    return request<PickBatch[]>(`/api/v1/picks/history?${params}`)
  },

  jobs: (query: JobsQuery = {}) => {
    const params = new URLSearchParams()
    if (query.limit != null) params.set('limit', String(query.limit))
    if (query.kind) params.set('kind', query.kind)
    if (query.task) params.set('task', query.task)
    if (query.failed) params.set('failed', 'true')
    const qs = params.toString()
    return request<JobsResponse>(`/api/v1/jobs${qs ? `?${qs}` : ''}`)
  },

  /** The Jobs table: every task's schedule, last run and next run. */
  scheduled: () => request<ScheduledResponse>('/api/v1/jobs/scheduled'),

  setSchedule: (task: TaskId, change: ScheduleChange) =>
    request<ScheduledResponse>(`/api/v1/jobs/scheduled/${task}`, { method: 'PUT', body: JSON.stringify(change) }),

  /** Run a task by hand with its own options; `refresh` fetches again what titles have. */
  runTask: (task: TaskId, refresh = false) =>
    request<Job>('/api/v1/jobs', { method: 'POST', body: JSON.stringify({ task, ...(refresh ? { refresh } : {}) }) }),

  syncSources: () => request<SyncLibrary[]>('/api/v1/sync/sources'),

  startSync: (options?: SyncOptions) =>
    request<Job>('/api/v1/jobs', {
      method: 'POST',
      body: JSON.stringify({ kind: 'sync', ...(options ? { options } : {}) }),
    }),

  enrichSources: () => request<EnrichSource[]>('/api/v1/enrich/sources'),

  startEnrich: (options: EnrichOptions) =>
    request<Job>('/api/v1/jobs', {
      method: 'POST',
      body: JSON.stringify({ kind: 'enrich', options }),
    }),

  cancelJob: () => request<Job>('/api/v1/jobs/current/cancel', { method: 'POST' }),

  /** A job with its failures and up to `limit` log lines after line `after`, at `level` or worse. */
  job: (id: number, after = 0, limit = 5000, level?: LogLevel) =>
    request<JobDetail>(`/api/v1/jobs/${id}?after=${after}&limit=${limit}${level ? `&level=${level}` : ''}`),

  /** Run a finished job's failures again: the sources that failed, for those titles. */
  retryFailed: (id: number) => request<Job>(`/api/v1/jobs/${id}/retry-failed`, { method: 'POST' }),

  gog: () => request<GogStatus>('/api/v1/gog'),
  gogLogin: (code: string) =>
    request<GogStatus>('/api/v1/gog/login', { method: 'POST', body: JSON.stringify({ code }) }),
  gogLogout: () => request<GogStatus>('/api/v1/gog', { method: 'DELETE' }),
  gogWindow: () => request<GogStatus>('/api/v1/gog/window', { method: 'POST' }),
  /** A GOG sign-in for the "Send to UpNext" bookmark: a one-time state for this device. */
  gogBrowser: () => request<GogBrowserLogin>('/api/v1/gog/browser', { method: 'POST' }),
  /** This install's key, which the bookmark carries back with the code. */
  gogBookmark: () => request<GogBookmarkKey>('/api/v1/gog/bookmark'),
  /** The bookmark's code back from GOG's page, with this device's state and the bookmark's key. */
  gogCallback: (state: string, code: string, key: string) =>
    request<GogStatus>('/api/v1/gog/callback', { method: 'POST', body: JSON.stringify({ state, code, key }) }),

  mergeTitles: (ids: number[]) =>
    request<GameView>('/api/v1/titles/merge', { method: 'POST', body: JSON.stringify({ ids }) }),
  splitCopy: (titleId: number, gameId: number) =>
    request<SplitResult>(`/api/v1/titles/${titleId}/split`, {
      method: 'POST',
      body: JSON.stringify({ game_id: gameId }),
    }),

  settings: () => request<Settings>('/api/v1/settings'),
  updateSettings: (patch: SettingsPatch) =>
    request<Settings>('/api/v1/settings', { method: 'PUT', body: JSON.stringify(patch) }),
  steamTest: () => request<SteamTestResult>('/api/v1/steam/test', { method: 'POST' }),

  /** Play later list, most recently saved first. */
  playLater: () => request<GameView[]>('/api/v1/play-later'),
  saveForLater: (id: number) => request<GameView>(`/api/v1/games/${id}/later`, { method: 'PUT' }),
  removeFromLater: (id: number) => request<GameView>(`/api/v1/games/${id}/later`, { method: 'DELETE' }),

  collections: () => request<Collection[]>('/api/v1/collections'),
  seriesOf: (id: number) => request<Collection | null>(`/api/v1/games/${id}/series`),

  /** A pick's action on its card; answers with the lineup as it stands after. */
  pickFeedback: (
    batchId: string,
    body: { title_id: number; action: FeedbackAction; text?: string; reasons?: { codes: string[]; note?: string } },
  ) => request<PickBatch>(`/api/v1/picks/${encodeURIComponent(batchId)}/feedback`, { method: 'POST', body: JSON.stringify(body) }),
  /** How well a pick fit its request, or null to take the rating back; answers with the lineup. */
  ratePick: (batchId: string, body: { title_id: number; verdict: PickVerdict | null; reason?: MissReason | null }) =>
    request<PickBatch>(`/api/v1/picks/${encodeURIComponent(batchId)}/rating`, { method: 'PUT', body: JSON.stringify(body) }),
  /** "It's too ___": the AI swaps in spares without the problem. */
  pickToo: (batchId: string, titleId: number, word: string) =>
    request<TooOutcome>(`/api/v1/picks/${encodeURIComponent(batchId)}/too`, {
      method: 'POST',
      body: JSON.stringify({ title_id: titleId, word }),
    }),
  /** "Switch with owned game" on a wishlisted pick: its owned alternate takes the slot. */
  pickSwitch: (batchId: string, titleId: number) =>
    request<PickBatch>(`/api/v1/picks/${encodeURIComponent(batchId)}/switch`, {
      method: 'POST',
      body: JSON.stringify({ title_id: titleId }),
    }),
  /** New spares for a lineup whose spares ran out, filling its empty slots. */
  pickMore: (batchId: string, sources?: Source[], input?: 'controller') =>
    request<PickBatch>(`/api/v1/picks/${encodeURIComponent(batchId)}/more`, {
      method: 'POST',
      body: JSON.stringify({ sources, ...(input ? { input } : {}) }),
    }),

  /** Versus "Help me decide": the AI picks one of two games. */
  decide: (ids: [number, number], prompt?: string) =>
    request<DecideResult>('/api/v1/versus/decide', { method: 'POST', body: JSON.stringify({ ids, prompt }) }),

  /** Settings' Test: a tiny round trip through the picks' AI, or with 'summaries' the summaries' one. */
  llmTest: (purpose?: 'summaries') =>
    request<LlmTestResult>(`/api/v1/llm/test${purpose ? `?purpose=${purpose}` : ''}`, { method: 'POST' }),
  llmModels: () => request<LlmModels>('/api/v1/llm/models'),
  llmCodex: () => request<CodexStatus>('/api/v1/llm/codex'),
  llmOpenAi: () => request<OpenAiKeyStatus>('/api/v1/llm/openai'),

  /** What the AI used over the last `days` days: totals, and by day, kind and model. */
  aiUsage: (days = 30) => request<AiUsageSummary>(`/api/v1/ai/usage?days=${days}`),
  /** On a subscription: the plan's usage bars, read now, and what UpNext's AI takes of them. */
  aiPlan: () => request<PlanSummary>('/api/v1/ai/plan'),
  /** The list prices the AI in use is priced at; read again when a week old. */
  aiPrices: () => request<PricesView>('/api/v1/ai/prices'),
  aiPricesRefresh: () => request<PricesView>('/api/v1/ai/prices/refresh', { method: 'POST' }),

  /** The backups folder, newest first. */
  backups: () => request<BackupsResponse>('/api/v1/backups'),
  /** A backup kept until it's deleted, with an optional note. */
  backUpNow: (note?: string) => request<Backup>('/api/v1/backups', { method: 'POST', body: JSON.stringify(note ? { note } : {}) }),
  /** Where a backup downloads from: everything, or without keys and sign-ins. */
  backupUrl: (name: string, keys: boolean) => `/api/v1/backups/${encodeURIComponent(name)}/download${keys ? '?keys=true' : ''}`,
  /** Start a job that puts a backup in place of the database (it backs the database up first). */
  restoreBackup: (name: string) => request<Job>(`/api/v1/backups/${encodeURIComponent(name)}/restore`, { method: 'POST' }),
  /** Upload a database and start a job that puts it in place of this one. */
  importDatabase: (file: File) =>
    request<Job>(`/api/v1/backups/import?name=${encodeURIComponent(file.name)}`, {
      method: 'POST',
      body: file,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  deleteBackup: (name: string) => request<void>(`/api/v1/backups/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  /** UpNext's shared game data: what is applied. */
  seed: () => request<SeedStatus>('/api/v1/seed'),
  /** The newest published shared data, read by the server without downloading it: for the setup wizard. */
  seedOffer: () => request<SeedManifest>('/api/v1/seed/offer', { method: 'POST' }),
  /** Download shared data and apply it; without an address, the newest published data. */
  applySeed: (url?: string) => request<SeedApplied>('/api/v1/seed', { method: 'POST', body: JSON.stringify(url ? { url } : {}) }),
  /** Apply a shared data file from this computer. */
  uploadSeed: (file: File) =>
    request<SeedApplied>(`/api/v1/seed/upload?name=${encodeURIComponent(file.name)}`, {
      method: 'POST',
      body: file,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  /** Stop using shared data: its summaries go, and your AI writes its own. */
  removeSeed: () => request<{ removed: number }>('/api/v1/seed', { method: 'DELETE' }),

  /** API tokens for other devices and scripts. */
  tokens: () => request<TokensResponse>('/api/v1/tokens'),
  /** Make a token; the answer is the only time it's seen in full. With a player, it acts for them
   *  alone, and needs their PIN when they have one. */
  createToken: (name: string, scope: TokenScope, playerId: number | null = null, pin: string | null = null) =>
    request<NewToken>('/api/v1/tokens', {
      method: 'POST',
      body: JSON.stringify({ name, scope, ...(playerId != null ? { player_id: playerId } : {}), ...(pin ? { pin } : {}) }),
    }),
  revokeToken: (id: number) => request<void>(`/api/v1/tokens/${id}`, { method: 'DELETE' }),

  /** Add a device: the code in effect, a new one, or end it. */
  pairing: () => request<Pairing | null>('/api/v1/pairing'),
  /** Whether a code shown here paired a device, or ended without one. */
  pairingOutcome: (code: string) => request<PairingOutcome>(`/api/v1/pairing/outcome?code=${encodeURIComponent(code)}`),
  /** A pairing code; `setup` makes its QR code open the setup wizard. `player` makes the
   *  device one player's, or the household's with 'shared'; left out, the server decides. A
   *  player with a PIN needs it, sent in the body so it stays out of the address. */
  startPairing: (then?: 'setup', player?: number | 'shared', pin?: string | null) => {
    const params = new URLSearchParams()
    if (then) params.set('then', then)
    if (player != null) params.set('player', String(player))
    const qs = params.toString()
    return request<Pairing>(`/api/v1/pairing${qs ? `?${qs}` : ''}`, {
      method: 'POST',
      ...(pin ? { body: JSON.stringify({ pin }) } : {}),
    })
  },
  /** Big screen in Steam and other launchers, on this computer only. */
  steamShortcut: () => request<SteamShortcut>('/api/v1/steam/shortcut'),
  addToSteam: () => request<SteamShortcut>('/api/v1/steam/shortcut', { method: 'POST' }),
  endPairing: () => request<void>('/api/v1/pairing', { method: 'DELETE' }),
  /** First-run setup: everything the wizard reads, at every step. */
  setup: () => request<SetupInfo>('/api/v1/setup'),
  /** Whether the wizard shows, cheaply: every page asks. */
  setupState: () => request<SetupState>('/api/v1/setup/state'),
  /** The step reached, and setup or the tour done, skipped or open again. */
  updateSetup: (change: SetupChange) => request<SetupState>('/api/v1/setup', { method: 'PUT', body: JSON.stringify(change) }),
  /** A sync, Fetch details or the shared data, run in the background while the wizard goes on. */
  setupBackground: (ask: SetupBackgroundAsk) =>
    request<SetupBackground>('/api/v1/setup/background', { method: 'POST', body: JSON.stringify(ask) }),
  /** Clear what was ticked, after a backup, before the wizard starts again. This computer only. */
  resetSetup: (body: SetupReset) => request<SetupResetDone>('/api/v1/setup/reset', { method: 'POST', body: JSON.stringify(body) }),
  /** A copy of GOG Galaxy's database from another computer. */
  galaxyUpload: (file: File) =>
    request<GalaxyUploaded>('/api/v1/galaxy/upload', {
      method: 'POST',
      body: file,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  /** The games added by hand, for each store whose library UpNext can't read. */
  epic: () => request<EpicStatus>('/api/v1/epic'),
  epicLogin: (code: string) =>
    request<EpicStatus>('/api/v1/epic/login', { method: 'POST', body: JSON.stringify({ code }) }),
  epicLogout: () => request<EpicStatus>('/api/v1/epic', { method: 'DELETE' }),
  byHand: () => request<ByHandLists>('/api/v1/library/by-hand'),
  /** Replace a store's list of games added by hand: one title a line. */
  byHandSave: (store: string, titles: string[]) =>
    request<ByHandSaved>(`/api/v1/library/by-hand/${encodeURIComponent(store)}`, { method: 'PUT', body: JSON.stringify({ titles }) }),
  /** What each AI feature would take for this library, with the AI in use. */
  aiEstimates: () => request<AiEstimates>('/api/v1/ai/estimates'),
  /** Needs no token: the pairing page asks it whether UpNext runs in its container. */
  health: () => request<{ app: string; version: string; build: string | null; container: boolean }>('/api/v1/health'),
  /** On the new device: trade the code for this browser's cookie. */
  pair: (code: string) => request<Paired>('/api/v1/pair', { method: 'POST', body: JSON.stringify({ code }) }),
  /** On the new device: ask the computer to allow this browser with a tap. */
  askToPair: () => request<PairAsked>('/api/v1/pair/request', { method: 'POST', body: JSON.stringify({}) }),
  /** The computer's answer; allowed, the response sets this browser's cookie. */
  pairAnswer: (asked: PairAsked) =>
    request<PairCollected>(`/api/v1/pair/request/${encodeURIComponent(asked.id)}`, { headers: { 'UpNext-Pair-Secret': asked.secret } }),
  /** On the computer: the devices asking, and the answer to one. */
  pairingRequests: () => request<WaitingDevices>('/api/v1/pairing/requests'),
  answerPairingRequest: (id: string, allow: boolean, playerId: number | null = null, pin: string | null = null) =>
    request<void>(`/api/v1/pairing/requests/${encodeURIComponent(id)}`, {
      method: 'POST',
      body: JSON.stringify({ allow, player_id: playerId, ...(pin ? { pin } : {}) }),
    }),
  /** The app window using another computer's UpNext, on this computer only. */
  nearbyComputers: () => request<NetworkNearby>('/api/v1/network/computers'),
  forgetComputer: (id: string) => request<void>(`/api/v1/network/computers/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  /** `id`: the install id it was found with, so another UpNext at that address is refused. */
  connectTo: (address: string, id: string | null = null) =>
    request<NetworkConnection>('/api/v1/network/connect', { method: 'POST', body: JSON.stringify({ address, id }) }),
  connection: () => request<NetworkConnection>('/api/v1/network/connect'),
  stopConnecting: () => request<void>('/api/v1/network/connect', { method: 'DELETE' }),

  /** Signing in with an identity provider: whether it's offered, and its name.
   *  The sign-in itself is a page load of `oidcLoginUrl`, which goes to the provider and back. */
  oidc: () => request<OidcStatus>('/api/v1/oidc'),
  /** Sign this browser out: its device token is revoked. The provider's own sign-in stays. */
  oidcLogout: () => request<void>('/api/v1/oidc/logout', { method: 'POST' }),
  /** "Link the next sign-in" to a player, with their PIN when they have one; this computer only. The
   *  account that signs in next waits to be confirmed (`confirmOidc`). */
  linkOidc: (id: number, pin: string | null) =>
    request<Player>(`/api/v1/players/${id}/oidc/link`, { method: 'POST', body: JSON.stringify(pin ? { pin } : {}) }),
  /** Confirm, with the player's PIN, that the account waiting is them; or stop waiting. */
  confirmOidc: (id: number, subject: string, pin: string | null) =>
    request<Player>(`/api/v1/players/${id}/oidc/confirm`, { method: 'POST', body: JSON.stringify({ subject, ...(pin ? { pin } : {}) }) }),
  disarmOidc: (id: number) => request<Player>(`/api/v1/players/${id}/oidc/link`, { method: 'DELETE' }),
  unlinkOidc: (id: number) => request<Player>(`/api/v1/players/${id}/oidc`, { method: 'DELETE' }),

  /** The players, and who this browser plays as. */
  players: () => request<PlayersResponse>('/api/v1/players'),
  addPlayer: (name: string, colour?: string) =>
    request<Player>('/api/v1/players', { method: 'POST', body: JSON.stringify({ name, ...(colour ? { colour } : {}) }) }),
  changePlayer: (id: number, change: { name?: string; colour?: string; default?: true }) =>
    request<Player>(`/api/v1/players/${id}`, { method: 'PUT', body: JSON.stringify(change) }),
  /** Set, change or remove (`newPin` null) your own PIN, with the one you have now. */
  setPin: (id: number, pin: string | null, newPin: string | null) =>
    request<void>(`/api/v1/players/${id}/pin`, { method: 'PUT', body: JSON.stringify({ pin, new_pin: newPin }) }),
  /** On the computer UpNext runs on only, after a backup. */
  removePlayer: (id: number, pin: string | null) =>
    request<{ backup: string; removed: string }>(`/api/v1/players/${id}`, {
      method: 'DELETE',
      body: JSON.stringify(pin ? { pin } : {}),
    }),
  /** Play as someone in this browser (the server sets its cookie). */
  choosePlayer: (id: number, pin?: string) =>
    request<Player>('/api/v1/players/current', { method: 'PUT', body: JSON.stringify({ player_id: id, ...(pin ? { pin } : {}) }) }),
  /** Hidden by me: every game you hid. Needs your PIN first when you have one (403). */
  hidden: () => request<HiddenTitle[]>('/api/v1/hidden'),
  unlockHidden: (pin: string) => request<void>('/api/v1/hidden/unlock', { method: 'POST', body: JSON.stringify({ pin }) }),
  /** Lock Hidden by me again in this browser before its ten minutes are up. */
  lockHidden: () => request<void>('/api/v1/hidden/unlock', { method: 'DELETE' }),
  hideGame: (id: number, scope: HideScope) =>
    request<HideAnswer>(`/api/v1/games/${id}/hide`, {
      method: 'PUT',
      body: JSON.stringify({ scope }),
    }),
  unhideGame: (id: number) => request<void>(`/api/v1/games/${id}/hide`, { method: 'DELETE' }),
  /** Your store connections, and every store account you own or are offered. */
  accounts: () => request<AccountsResponse>('/api/v1/accounts'),
  /** An account you own: who it's offered to, and each other player's part. */
  setAccount: (id: number, change: { sharing?: Sharing; players?: { player_id: number; offered?: boolean; plays_on?: boolean }[] }) =>
    request<StoreAccount>(`/api/v1/accounts/${id}`, { method: 'PUT', body: JSON.stringify(change) }),
  /** Link a library for you: a Steam account, a sign-in, a Heroic folder or a Galaxy database. */
  addConnection: (kind: string, settings: Record<string, unknown>) =>
    request<StoreConnection>('/api/v1/connections', { method: 'POST', body: JSON.stringify({ kind, settings }) }),
  /** Change one of your connections' settings; null clears one. */
  setConnection: (id: number, settings: Record<string, unknown>) =>
    request<StoreConnection>(`/api/v1/connections/${id}`, { method: 'PUT', body: JSON.stringify({ settings }) }),
  /** Use the games in an account offered to you, or not. */
  useAccount: (id: number, uses: boolean) =>
    request<StoreAccount>(`/api/v1/accounts/${id}/use`, { method: 'PUT', body: JSON.stringify({ uses }) }),
  aiAccounts: () => request<AiAccount[]>('/api/v1/ai-accounts'),
  /** An AI account you own: who it's offered to, or the library's own AI work on it. */
  setAiAccount: (id: number, change: { sharing?: Sharing; offered_to?: number[]; library?: true }) =>
    request<AiAccount>(`/api/v1/ai-accounts/${id}`, { method: 'PUT', body: JSON.stringify(change) }),
  /** The AI your picks, Help me decide and Second Chance run on. */
  useAiAccount: (id: number) => request<AiAccount>(`/api/v1/ai-accounts/${id}/use`, { method: 'PUT' }),

  /** Backlog verdicts: your own, the form, the consent, sending. */
  verdicts: () => request<VerdictsOverview>('/api/v1/verdicts'),
  verdictContract: () => request<VerdictContract>('/api/v1/verdicts/vocab'),
  verdictForm: (id: number) => request<VerdictForm>(`/api/v1/verdicts/draft/${id}`),
  verdictConsent: () => request<VerdictConsent>('/api/v1/verdicts/consent'),
  acceptVerdictConsent: (version: number) =>
    request<VerdictConsent>('/api/v1/verdicts/consent', { method: 'POST', body: JSON.stringify({ version }) }),
  saveVerdict: (id: number, answers: VerdictAnswers) =>
    request<MyVerdict>(`/api/v1/verdicts/${id}`, { method: 'PUT', body: JSON.stringify(answers) }),
  sendVerdict: (id: number, answers: VerdictAnswers) =>
    request<VerdictSent>(`/api/v1/verdicts/${id}/send`, { method: 'POST', body: JSON.stringify(answers) }),
  /** Take a verdict down; null for a draft never sent, which is forgotten. */
  withdrawVerdict: (id: number) => request<{ verdict: MyVerdict | null }>(`/api/v1/verdicts/${id}`, { method: 'DELETE' }),
  withdrawAllVerdicts: () => request<{ withdrawn: number; forgot_key: boolean }>('/api/v1/verdicts/withdraw-all', { method: 'POST' }),
  refreshVerdicts: () =>
    request<{ listed: number; added: number; changed: number; unmatched: number }>('/api/v1/verdicts/refresh', { method: 'POST' }),
  /** Other players' verdicts: the snapshot kept, and a game's. */
  communityVerdicts: () => request<CommunityStatus>('/api/v1/verdicts/community'),
  gameVerdicts: (id: number) => request<GameCommunity>(`/api/v1/games/${id}/verdicts`),
  reportVerdict: (verdictId: string, reason: string) =>
    request<Record<string, never>>('/api/v1/verdicts/reports', { method: 'POST', body: JSON.stringify({ verdict_id: verdictId, reason }) }),
}
