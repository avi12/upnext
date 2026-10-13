import type { components, operations } from './generated/api'

// API types come from the generated file (docs/openapi.json, `npm run gen:api`)
// where it says exactly what these said. The rest stay hand-written because
// the document is looser than the UI needs: an `Option` field is optional
// there (`x?: T | null`), and a store or launcher is a plain string.

/** In the order the library sorts by status (schema v18). */
export type Status = components['schemas']['Status']

/** A status you set. done/shelved/abandoned are for games with an ending,
 *  still_playing/burnt_out for endless ones, hidden ("Not for me") for both. */
export type OverrideStatus = 'done' | 'shelved' | 'abandoned' | 'still_playing' | 'burnt_out' | 'hidden'

/** Whether a game has an ending to reach. */
export type GameKind = 'ending' | 'endless'

/** Why a game was put down, the latest time you said. */
export interface StopReasons {
  /** Codes from GET /api/stop-reasons, in its order. */
  codes: string[]
  note: string | null
  source: 'menu' | 'pick' | 'second_chance'
  at: number
}

/** Whether what stopped you has changed, by the reviews written since. */
export type SecondChanceVerdict = components['schemas']['SecondChanceVerdict']
export type SecondChanceAnswer = 'willing' | 'still_no' | 'not_for_me'

/** Second Chance's "Now" for a game, and the page's answer. */
export interface SecondChanceNote {
  /** What player reviews since you stopped say about why, by the AI. */
  now: string | null
  verdict: SecondChanceVerdict | null
  /** Its gist, as picks see it. */
  line: string | null
  written_at: number | null
  model: string | null
  /** Written before your reasons or the reviews last changed. */
  outdated: boolean
  answer: SecondChanceAnswer | null
  answered_at: number | null
}

/** One game on the Second Chance page. */
export interface SecondChanceEntry {
  game: GameView
  /** When it was put down. */
  stopped_at: number | null
  /** Player reviews written since then, which "Now" is written from. */
  reviews_since: number
  /** The next Second chances run writes its "Now". */
  due: boolean
}

export interface SecondChancesResponse {
  games: SecondChanceEntry[]
  ai: { available: boolean; reason: string | null }
  task: { schedule: string; enabled: boolean; last_run: Job | null; next_run_at: number | null }
}

/** The AI's ratings of a hidden gem, each 0 to 3. */
export interface GemRating {
  studio: number
  mechanics: number
  play: number
  /** A party game: kept off the shelf. */
  party: boolean
  /** What makes it unusual, in one sentence. */
  line: string
  prompt_version: number
  model: string | null
  written_at: number
}

export interface Gem {
  game: GameView
  /** Null for a game in the pool not rated yet. */
  rating: GemRating | null
  /** The three ratings together, 0 to 9. */
  score: number | null
  /** Its three rarest tags, with how many games in the library carry each. */
  rarest: [string, number][]
}

/** GET /api/gems: the Hidden gems shelf and the pool behind it. */
export interface GemsResponse {
  gems: Gem[]
  pool: number
  rated: number
  /** Games the next Find hidden gems run rates. */
  unrated: number
  party: number
  /** With `?all=true`: every game in the pool, ranked ones first. */
  ranked?: Gem[]
  ai: { available: boolean; reason: string | null }
  task: { schedule: string; enabled: boolean; last_run: Job | null; next_run_at: number | null }
  rules: { min_positive: number; min_reviews: number; max_hours: number; count: number }
}

export type WishSource = components['schemas']['WishSource']
export type WishState = components['schemas']['WishState']

/** A game you don't own on the recommended wishlist, with what Steam's store says. */
export interface WishlistEntry {
  id: number
  appid: number
  name: string
  source: WishSource
  /** Why it's suggested: your history with its series. Null for Steam's and ones you added. */
  reason: string | null
  state: WishState
  /** On your Steam wishlist now. */
  on_steam: boolean
  series_key: string | null
  /** The series it's from, by name. */
  series: string | null
  batch_id: string | null
  title_id: number | null
  hltb_main: number | null
  added_at: number
  updated_at: number
  wishlisted_at: number | null
  /** Steam's app type; null until its store details are fetched. */
  type: string | null
  short_description: string | null
  genres: string[]
  developers: string[]
  release_date: string | null
  header_image: string | null
  portrait: string | null
  review_pct: number | null
  review_total: number | null
  review_desc: string | null
  tags: string[]
}

/** GET /api/wishlist. With the wishlist off, `enabled` is false and everything else is empty. */
export interface WishlistResponse {
  enabled: boolean
  suggested: WishlistEntry[]
  wishlist: WishlistEntry[]
  dismissed: number
  owned_elsewhere: number
  in_library: number
  steam_account?: boolean
  task?: { schedule: string; enabled: boolean; last_run: Job | null; next_run_at: number | null }
  rules?: { series_hours: number; min_positive: number; min_reviews: number; per_series: number }
}

/** A genre or mechanic a game is a way into (GET /api/new-to-you/{id}). */
export interface WayIn {
  /** The profile subject: "tag:Deckbuilding". */
  about: string
  tag: string
  /** "deckbuilders" */
  phrase: string
  kind: 'genre' | 'mechanic'
}

/** One reason a game can be put down for (GET /api/stop-reasons). */
export interface StopReason {
  code: string
  /** life reasons are never held against similar games; game reasons are checked against reviews. */
  group: 'life' | 'game'
  label: string
  aspects: string[]
}

export type Source = 'steam' | 'epic' | 'gog' | 'amazon'
/** Who brought a copy in: a launcher, a store's own account or launcher
 *  (`gog`, `epic`, `origin`, `uplay`), or a list added by hand (`manual`). */
export type Launcher = 'steam' | 'heroic' | 'galaxy' | 'gog' | 'epic' | 'origin' | 'uplay' | 'manual'

export interface Copy {
  game_id: number
  source: Source
  source_id: string
  launcher: Launcher
  /** The store's own name for this copy (may carry an edition suffix). */
  name: string
  /** "Remastered", "Ultimate Edition", ... or null for the plain release. */
  edition: string | null
  installed: boolean
  playtime_minutes: number
  playtime_known: boolean
  last_played: number
  run_url: string
  store_url: string
}

/** "native" when a Linux build exists, else the ProtonDB tier. */
export type LinuxTier = 'native' | 'platinum' | 'gold' | 'silver' | 'bronze' | 'borked' | 'pending'

/** Library filter: ok = native/platinum/gold, issues = silver/bronze/borked, unknown = no data or pending. */
export type LinuxFilter = 'ok' | 'native' | 'issues' | 'unknown'

export interface ReviewText {
  source: Source | string
  /** Steam thumbs; null for GOG. */
  positive: boolean | null
  /** GOG stars 1..5; null for Steam. */
  rating: number | null
  votes: number
  hours: number | null
  title: string | null
  text: string
  /** When it was posted, Unix seconds; null for reviews stored before 2026-09-30. */
  posted?: number | null
}

export type Verdict = 'praised' | 'criticized' | 'divisive'

/** Review tags from a fixed vocabulary (src/review_tags.rs; values listed in docs/api/review-tags.md). */
export interface ReviewTags {
  /** One value per scale the reviews speak to, e.g. { difficulty: 'hard', sessions: 'short' }. */
  scales: Record<string, string>
  mood: string[]
  skills: string[]
  /** Praised first, then criticized, then divisive. */
  aspects: { aspect: string; verdict: Verdict }[]
  /** Short points the vocabulary can't express, e.g. "slow first hours". */
  notes: string[]
}

/** One value of a review-tag scale or set, with how many titles have it. */
export interface ReviewTagValue {
  value: string
  /** Plain label, as in the merged tags: "Short sessions". */
  label: string
  count: number
}

/** An ordered scale (lowest first) or an unordered set of review tags. */
export interface ReviewTagGroup {
  name: string
  /** Its name on the game page, "Learning curve". */
  title: string
  means: string
  values: ReviewTagValue[]
  /** Titles with any value of it: how much of the library a filter on it covers. */
  known: number
  /** Sets only: at most this many values per title. */
  max?: number
}

/** GET /api/review-tags: the vocabulary, with counts over the library. */
export interface ReviewVocabulary {
  /** Titles with review tags. */
  tagged: number
  scales: ReviewTagGroup[]
  /** mood, then skills. */
  sets: ReviewTagGroup[]
  aspects: { name: string; praised: number; criticized: number; divisive: number }[]
}

export interface GameView {
  /** Title id (stable across stores). Per-store rows live in `copies[].game_id`. */
  id: number
  /** Preferred copy's source/launcher (installed first, then Steam, GOG, Epic, Amazon). */
  source: Source
  source_id: string
  launcher: Launcher
  owned_on: Source[]
  primary_source: Source
  /** Distinct editions across copies; empty when every copy is the plain release. */
  editions: string[]
  copies: Copy[]
  appid: number | null
  steam_matched: boolean
  /** True if any copy is installed. */
  installed: boolean
  name: string
  /** Max over copies that track playtime. */
  playtime_minutes: number
  playtime_known: boolean
  playtime_2weeks: number
  last_played: number
  status: Status
  /** The status you set, as it stands now: a Shelved mark untouched for long
   *  enough reads abandoned, a Burnt out one past its break still_playing, and
   *  a mark on a game played since is null. */
  override: OverrideStatus | null
  override_note: string | null
  kind: GameKind
  /** Set on the sheet: the game can be played endlessly (true) or has an ending (false); null when inferred. */
  no_ending: boolean | null
  /** Played at least the staple hours (100 by default). */
  staple: boolean
  /** When UpNext found it shelved or abandoned (null when you set it). */
  shelved_at: number | null
  abandoned_at: number | null
  /** When a Burnt out game comes back to picks. */
  burnt_out_until: number | null
  stop_reasons: StopReasons | null
  /** Times it was turned down in picks while it has this status: one spends an abandoned game's second chance. */
  rejections: number
  /** Abandoned because it was turned down twice in picks. */
  abandoned_by_picks: boolean
  /** The last "It's too ___" said about it in a lineup: a light note. */
  too: { word: string; at: number } | null
  /** The last time it was chosen from a lineup with Play, and what the lineup was asked for. */
  chosen: { at: number; prompt: string } | null
  /** Your own rating of the game, 1 ("Disliked it") to 5 ("Loved it"), and when you gave it. */
  my_rating?: number | null
  rated_at?: number | null
  /** The latest rating it had as a pick, from any lineup, with what that lineup was asked for. */
  pick_rating?: (PickRating & { prompt: string }) | null
  /** Shelved again from the Second Chance page: turned down once in picks, it's abandoned for good. */
  second_try: boolean
  /** The Second Chance page's "Now" and answer, once it has either. */
  second_chance: SecondChanceNote | null
  /** It was on the wishlist before you got it; only GET /api/games/{id} has it. */
  from_wishlist?: { reason: string | null; source: WishSource; wishlisted_at: number }
  /** How you hid it: from yourself, from the others, or both; null when you haven't. */
  hidden_scope?: HideScope | null
  /** One of its copies is in a store account of your own, so you can hide it from the others. */
  own_copy?: boolean
  type: string | null
  short_description: string | null
  genres: string[]
  categories: string[]
  /** Merged, de-duplicated tags from every source (Steam, review tags, GOG, RAWG, IGDB, PCGamingWiki). */
  tags: string[]
  tags_by_source: Record<string, string[]>
  /** IGDB themes + perspectives + game modes, plus PCGamingWiki taxonomy. */
  themes: string[]
  header_image: string | null
  art_square: string | null
  metacritic: number | null
  release_date: string | null
  developers: string[]
  review_score: number | null
  review_desc: string | null
  review_positive: number | null
  review_total: number | null
  review_pct: number | null
  hltb_main: number | null
  hltb_extra: number | null
  hltb_complete: number | null
  hltb_url: string | null
  gog_rating: number | null
  gog_rating_count: number | null
  igdb_rating: number | null
  igdb_rating_count: number | null
  /** RAWG user rating 0..5. */
  rawg_rating: number | null
  rawg_ratings_count: number | null
  /** Source pages. RAWG's free tier requires a visible backlink wherever its data is shown. */
  rawg_url: string | null
  pcgw_url: string | null
  wikidata_url: string | null
  /** From PCGamingWiki. */
  series: string[]
  /** Steam lists a Linux build. */
  linux_native: boolean | null
  protondb_tier: string | null
  protondb_trending: string | null
  /** ProtonDB report count. */
  protondb_total: number | null
  /** Native build or ProtonDB tier; null = unknown. */
  linux: LinuxTier | null
  /** Most helpful player reviews (Steam thumbs, GOG stars). */
  review_texts: ReviewText[]
  /** People say: an AI-written critical look at the reviews (strengths, weaknesses, what sets it apart); null until generated. */
  digest: string | null
  /** Vibes: what playing it feels like, written from the same reviews; null until then, or when reviews don't say. */
  vibes: string | null
  /** What the reviews say the game is like, written with the digest; null until then. */
  review_tags: ReviewTags | null
  /** The date (YYYY-MM-DD) of UpNext's shared data that People say, Vibes and the
   *  review tags came from; null when this install wrote them. */
  seed_date: string | null
  /** What other backlog players say about it, when they're on and
   *  it has at least 3 verdicts. */
  backlog?: Backlog | null
  enriched: boolean
  store_url: string
  run_url: string
  /** Original release year (Wikidata); null = use release_date. */
  first_year: number | null
  /** Unix seconds when the status override was set; null = none. */
  override_at: number | null
  /** Unix seconds when saved to Play later; null = not saved. */
  saved_at: number | null
  /** YouTube trailer, or a YouTube search when no trailer is known. */
  trailer_url: string | null
  /** Gameplay screenshots (Steam first, else IGDB). */
  screenshots: Screenshot[]
  /** Steam portrait (2:3) art, from the store's asset list. */
  portrait_url: string | null
  /** Steam wide hero art. */
  hero_url: string | null
  /** Steam's word for how well a pad works with it: `full` or `partial`; null when Steam doesn't say. */
  controller_support?: string | null
}

export interface SourceInfo {
  source: string
  launcher: Launcher | string
  label: string
  configured: boolean
  count: number
  last_sync: number | null
  note?: string | null
}

export interface GogStatus {
  connected: boolean
  user_id: string | null
  username: string | null
  auth_url: string
  /** A native GOG login window is open; poll until it clears. */
  login_pending: boolean
  /** Why the last login window attempt failed. */
  login_error: string | null
}

export interface Profile {
  steam_id: string
  persona_name: string | null
  avatar: string | null
  profile_url: string | null
  last_sync: number | null
  /** Titles. */
  game_count: number
  /** Per-store rows. */
  copy_count: number
  sources: SourceInfo[]
}

export interface Config {
  model: string
  sampled_minutes: number
  max_candidates: number
  db_path: string
  /** The configured AI backend can run (the name is from when it was always Claude). */
  claude_available: boolean
  /** Why it can't, when it can't ("Codex isn't signed in: run `codex login` …"). */
  llm_error: string | null
  llm_backend: string
  steam_configured: boolean
  heroic_config_dir: string | null
  galaxy_db: string | null
  /** Effective Linux compatibility mode (ProtonDB tiers in picks and the UI). */
  linux_active: boolean
  rawg_configured: boolean
}

export type NameCount = components['schemas']['NameCount']

export interface Stats {
  /** Titles. */
  total: number
  /** Store copies (rows). */
  copies: number
  /** Titles owned on 2+ stores. */
  multi_store: number
  by_status: Record<Status, number>
  /** Games per store, Galaxy's stores (origin, xboxone…) included. */
  by_source: Record<string, number>
  installed: number
  candidates: number
  /** Your staples. */
  staples: number
  /** Games you rated. */
  rated: number
  enriched: number
  unenriched: number
  with_reviews: number
  with_tags: number
  with_hltb: number
  with_review_texts: number
  with_digests: number
  /** Titles with a Wikidata cross-reference. */
  with_xref: number
  with_rawg: number
  with_pcgw: number
  /** "native" | ProtonDB tier | "unknown" -> count (excludes not-games). */
  by_linux: Record<string, number>
  total_playtime_minutes: number
  genres: NameCount[]
  tags: NameCount[]
  top_played: GameView[]
}

export type SortKey =
  | 'name'
  | 'playtime'
  | 'last_played'
  | 'review'
  | 'hltb'
  | 'release'
  | 'metacritic'
  | 'rating'
  | 'status'
  | 'source'
  | 'linux'

export type RatingFilter = 'rated' | 'unrated' | 'high' | 'low'

export interface GamesQuery {
  status?: Status[]
  /** Only your staples. */
  staple?: boolean
  source?: string[]
  installed?: boolean
  /** Only games Steam says a pad works with, fully or in part. */
  controller?: boolean
  q?: string
  genre?: string
  tag?: string
  linux?: LinuxFilter
  /** Review-tag facets a title must all have, "sessions:short". */
  facet?: string[]
  /** Aspects players praised. */
  praised?: string[]
  /** Aspects players didn't criticize or split on (titles with review tags only). */
  not_criticized?: string[]
  /** Your own rating: any, none, 4 or 5 stars, or 1 or 2. */
  rating?: RatingFilter
  /** What most other backlog players said. */
  backlog?: BacklogFilter
  sort?: SortKey
  order?: 'asc' | 'desc'
  limit?: number
  offset?: number
}

/** The library's backlog verdicts filter: most players said it respects limited time, or clicks in the first hour. */
export type BacklogFilter = 'respects_time' | 'clicks_fast'

export interface GamesResponse {
  total: number
  items: GameView[]
}

/** What was done with a pick on its card. `same_problem`: taken off by the
 *  same "It's too ___" call as another pick. */
export type FeedbackAction = components['schemas']['FeedbackAction']

export interface PickFeedback {
  action: FeedbackAction
  /** The word for `too` and `same_problem`; how it went (an override) for `started`. */
  text: string | null
  at: number
}

/** How well a pick fit its lineup's request: "Good pick" or "Not a fit". */
export type PickVerdict = components['schemas']['PickVerdict']
/** Why a pick was not a fit, when you said. */
export type MissReason = components['schemas']['MissReason']

/** A pick's rating on its card: not an action, so the pick stays where it is. */
export interface PickRating {
  verdict: PickVerdict
  /** Only with not_a_fit, and optional. */
  reason: MissReason | null
  at: number
}

interface PickBase {
  id: number
  appid: number | null
  name: string
  reason: string
  /** Where it shows in the lineup, from 0; null for a spare or a pick taken off. */
  slot: number | null
  /** The last thing done with it on its card. */
  feedback: PickFeedback | null
  /** How well it fit the request, as rated on its card. */
  rating?: PickRating | null
  /** What other backlog players say, the parts the request asks about first:
   *  "Backlog players: clicks fast, fine in short sessions (23)". */
  backlog?: string | null
}

/** A pick of a game in your library. */
export interface OwnedPick extends PickBase {
  owned?: true
  game: GameView
}

/** A game you don't own, as picks show it: a wishlisted pick, or a lineup's suggestion. */
export interface WishGame {
  /** Its wishlist entry (`/api/wishlist/{id}`). */
  id: number
  appid: number
  name: string
  source: WishSource
  /** Where it stands now: a suggestion answered since shows its answer. */
  state: WishState
  on_steam: boolean
  short_description: string | null
  genres: string[]
  tags: string[]
  release_date: string | null
  header_image: string | null
  portrait: string | null
  review_pct: number | null
  review_total: number | null
  review_desc: string | null
  hltb_main: number | null
}

/** A wishlisted game you don't own standing in for a pick: `id` is
 *  minus its wishlist entry's id, and "Switch with owned game" puts `alternate` in its slot. */
export interface WishlistedPick extends PickBase {
  owned: false
  game: null
  wishlist: WishGame
  alternate?: { id: number; name: string; reason: string; game: GameView | null } | null
}

export type Pick = OwnedPick | WishlistedPick

/** A game you don't own that the AI suggested with a lineup, checked against Steam's store and your library. */
export interface Suggestion {
  game: WishGame
  reason: string
}

/** What a lineup holds beyond the picks on screen. */
interface Lineup {
  /** How many picks the lineup shows; a slot with no pick waits for new spares. */
  shown: number
  /** Spares not used yet, best first. */
  spares: Pick[]
  /** Picks taken off, in the order they went, each with its `feedback`. */
  removed: Pick[]
  /** Picked without AI, from UpNext's own ranking: what it was ranked by, in plain words. Null when the AI picked. */
  ranked_by: string[] | null
  /** The discovery mode it was picked in, or null. */
  mode?: PickMode | null
  /** A game you don't own, right down your alley, or null. */
  suggestion?: Suggestion | null
  /** The routine it was picked for, as it was then, or null. */
  routine?: Routine | null
}

/** A named session: when, who with, where, on what and for how long. */
export type Routine = components['schemas']['Routine']
/** A routine as sent to make or replace one: only the name is required. */
export type RoutineBody = components['schemas']['RoutineBody']
/** GET /api/routines: every routine, and the id of the one that fits now. */
export type Routines = components['schemas']['Routines']
export type RoutineDay = components['schemas']['Day']
export type RoutineTime = components['schemas']['Time']
export type RoutineWho = components['schemas']['Who']
export type RoutinePlace = components['schemas']['Place']
export type RoutineInput = components['schemas']['Input']

/** The discovery pick modes, "New to me" and "A hidden gem", and the dice
 *  button's random draw from the games off your radar. */
export type PickMode = 'new_to_you' | 'gems' | 'random'

/** POST /api/picks/{batch_id}/too. */
export interface TooOutcome {
  batch: PickBatch
  /** The turned-down pick first, then any other with the same problem. `into` is null when no spare fit. */
  swaps: { slot: number; out: number; into: number | null }[]
  /** Why the AI couldn't check the spares; the swap then came from the word alone. */
  ai_error: string | null
}

export interface PickResult extends Lineup {
  batch_id: string
  prompt: string
  created_at: number
  /** Null for a lineup picked without AI. */
  model: string | null
  duration_ms: number
  /** Charged for its calls: an API key or a hosted provider. */
  cost_usd: number | null
  /** Its calls at list prices, charged or not; on a subscription, what they'd cost on the API. */
  api_usd: number | null
  candidates_considered: number
  attempts: number
  /** Tokens of the pick's calls (both when the first answer was turned down); null when the backend didn't say. */
  usage: Usage | null
  picks: Pick[]
}

export interface PickBatch extends Lineup {
  batch_id: string
  prompt: string
  created_at: number
  /** Null for a lineup picked without AI. */
  model: string | null
  /** The server sends these for every batch; LineupMeta reads them as a PickResult's. */
  duration_ms: number | null
  candidates_considered: number | null
  attempts: number | null
  /** Null for batches from before schema v15, and after a year. */
  usage: Usage | null
  cost_usd: number | null
  api_usd: number | null
  picks: Pick[]
}

export type JobKind = 'sync' | 'enrich' | 'backup' | 'restore' | 'import' | 'taste' | 'verdicts'
export type JobStatus = components['schemas']['JobStatus']

/** A scheduled task: one row of Settings › Jobs. */
export type TaskId =
  | 'sync'
  | 'details'
  | 'reviews'
  | 'digests'
  | 'series_guides'
  | 'second_chances'
  | 'hidden_gems'
  | 'wishlist'
  | 'taste'
  | 'hltb'
  | 'backup'
  | 'verdicts'

/**
 * What a sync reads: each library on or off by id (the ids come from
 * `GET /api/sync/sources`; a missing id is on), plus the Galaxy database to
 * read and whether to force the local Steam client.
 */
export interface SyncOptions {
  [library: string]: boolean | string | null | undefined
  steam_local?: boolean
  /** Galaxy is skipped without one. */
  galaxy_db?: string | null
}

/** One library a sync can read, from `GET /api/sync/sources`. */
export interface SyncLibrary {
  id: string
  label: string
  /** Why the current settings keep it from syncing, e.g. "not connected". */
  blocked: string | null
}

/**
 * What an enrich run fetches: each source's option by id (the ids come from
 * `GET /api/enrich/sources`; a missing id takes its default), plus refresh
 * and limit.
 */
export interface EnrichOptions {
  [source: string]: boolean | number | null | undefined
  /** Fetch again for games that already have it. */
  refresh?: boolean
  limit?: number | null
}

/** One enrich source's option, from `GET /api/enrich/sources`. */
export interface EnrichSource {
  id: string
  label: string
  /** On unless turned off. */
  default: boolean
  /** A word on cost, e.g. "slow" or "uses the AI". */
  note: string | null
  /** Why the current settings keep it from running, e.g. "needs a key". */
  blocked: string | null
}

/** Where a job was started: by hand in the UI, by its schedule, or `upnext sync`/`enrich`/`backup` in a terminal. */
export type JobSource = components['schemas']['JobSource']

export type LaneState = components['schemas']['LaneState']

/** One lane of an enrich run: a service taking every title in turn, or a pass over the library. */
export interface LaneProgress {
  name: string
  done: number
  total: number
  failed: number
  state: LaneState
  /** The title it's on. */
  current: string | null
  /** What it's waiting on or why it stopped: "Steam asked us to slow down". */
  note: string | null
  /** While waiting: when it tries again (Unix seconds). */
  retry_at: number | null
}

/** Tokens of one AI call or of several added up. `input_tokens` is all of the input. */
export interface Usage {
  input_tokens: number
  output_tokens: number
  cache_read_tokens: number
  cache_write_tokens: number
  /** Of `cache_write_tokens`, those kept for an hour (they cost more). */
  cache_write_1h_tokens: number
}

/** A job's AI calls added up (docs/api/ai-usage.md). */
export interface JobAiUsage extends Usage {
  calls: number
  failed: number
  /** Money charged: an API key or a hosted provider. */
  cost_usd: number | null
  /** At list prices, charged or not. */
  api_usd: number | null
  /** What the Claude CLI said its calls would cost on the API. Kept, not shown. */
  would_cost_usd: number | null
  items: number
  duration_ms: number
  /** While it runs: tokens in and out the whole job should take. */
  estimated_tokens: number | null
}

export interface Job {
  id: number
  kind: JobKind
  /** The scheduled task it ran as, or null for other work (a retry, one source). */
  task: TaskId | null
  source: JobSource
  status: JobStatus
  started_at: number
  finished_at: number | null
  heartbeat_at: number
  total: number
  done: number
  current: string | null
  message: string
  /** Titles (or games) per minute lately; null until there's enough to go on. */
  per_minute: number | null
  /** Seconds left at that rate. */
  eta_secs: number | null
  lanes: LaneProgress[]
  /** Failures so far. */
  failed: number
  /** What it did, once done: "2,000 games, 3 new". */
  summary: string | null
  error: string | null
  options: Partial<EnrichOptions & SyncOptions> | null
  cancel_requested: boolean
  /** Its AI calls' tokens, when it made any. */
  ai_usage: JobAiUsage | null
}

export interface JobsResponse {
  current: Job | null
  recent: Job[]
}

/** What came of a lineup, for History's filter. */
export type HistoryOutcome = NonNullable<NonNullable<operations['history']['parameters']['query']>['outcome']>

/** Which past lineups `GET /api/picks/history` lists, besides how many. */
export interface HistoryQuery {
  /** Lineups skipped first, for the next page. */
  offset?: number
  order?: 'newest' | 'oldest'
  /** Only lineups made at or after this time (Unix seconds). */
  since?: number
  /** Words in the request or a game the lineup showed. */
  q?: string
  outcome?: HistoryOutcome
}

/** Which finished jobs `GET /api/jobs` lists. */
export interface JobsQuery {
  limit?: number
  kind?: JobKind
  /** A task, or 'none' for jobs that weren't one. */
  task?: TaskId | 'none'
  /** Only jobs that failed or finished with failures. */
  failed?: boolean
}

/** One row of the Jobs table, from `GET /api/jobs/scheduled`. */
export interface ScheduledTask {
  task: TaskId
  label: string
  /** "off", "daily", "weekly", "monthly" or "after:<task>". */
  schedule: string
  uses_ai: boolean
  /** Someone set it, rather than it being the default. */
  chosen: boolean
  /** On or off by library (sync) or source (details); null for tasks without options. */
  options: Record<string, boolean> | null
  /** Backups kept (backup only). */
  keep: number | null
  /** At most this many titles a run (the AI jobs), or null. */
  limit: number | null
  last_run: Job | null
  /** When it runs next; in the past means as soon as nothing else runs. */
  next_run_at: number | null
  /** Or: when this task next ends done. */
  next_after: TaskId | null
}

export interface ScheduledResponse {
  /** Schedules on (the `auto_sync` setting). */
  enabled: boolean
  /** Nothing runs by itself before the first sync. */
  waiting_for_first_sync: boolean
  tasks: ScheduledTask[]
}

export interface ScheduleChange {
  schedule?: string
  options?: Record<string, boolean>
  keep?: number
  /** The AI jobs: at most this many titles a run; 0 for no limit. */
  limit?: number
}

/** First-run setup: what the wizard reads, and where it got to. */
export type SetupInfo = components['schemas']['Setup']
/** A GOG sign-in started for the "Send to UpNext" bookmark. */
export type GogBrowserLogin = components['schemas']['GogBrowserLogin']
/** This install's key for the "Send to UpNext" bookmark (security review L10). */
export type GogBookmarkKey = components['schemas']['GogBookmark']
export type SetupState = components['schemas']['SetupState']
/** What "Restart the setup wizard" clears first. */
export type SetupReset = components['schemas']['ResetBody']
export type SetupResetDone = components['schemas']['ResetDone']
export type SetupChange = components['schemas']['SetupChange']
export type SetupProgress = components['schemas']['SetupProgress']
/** What the wizard runs in the background, and what it waits on. */
export type SetupBackgroundAsk = components['schemas']['SetupBackgroundAsk']
export type SetupBackground = components['schemas']['SetupBackground']
export type SeedProgress = components['schemas']['SeedProgress']
export type GalaxyUploaded = components['schemas']['GalaxyUploaded']
/** Games added by hand, for the stores whose library UpNext can't read. */
export type ByHandLists = components['schemas']['ByHandLists']
export type ByHandSaved = components['schemas']['ByHandSaved']
/** The Epic sign-in: Settings › Accounts and the setup's store steps. */
export type EpicStatus = components['schemas']['EpicStatus']
export type AiEstimates = components['schemas']['Estimates']
export type FeatureEstimate = components['schemas']['FeatureEstimate']
export type Amount = components['schemas']['Amount']

export type LogLevel = 'info' | 'warn' | 'error'

export interface JobLogLine {
  id: number
  at: number
  level: LogLevel
  lane: string | null
  message: string
}

export interface JobFailure {
  lane: string
  /** What failed: "Steam reviews", "store search". */
  source: string
  options: string[]
  title_id: number | null
  /** The title's name, or what else it was about (a series). */
  name: string | null
  error: string
  at: number
}

/** `GET /api/jobs/{id}`: the job, its failures and a page of its log. */
export interface JobDetail extends Job {
  failures: JobFailure[]
  /** Whether "retry-failed" has titles to run again. */
  can_retry: boolean
  log: JobLogLine[]
  log_more: boolean
}

export type SettingSource = 'app' | 'env' | 'steam client' | 'default' | null

export interface Settings {
  steam_api_key_set: boolean
  steam_api_key_source: SettingSource
  steam_id: string | null
  steam_id_source: SettingSource
  steam_persona: string | null
  steam_avatar: string | null
  model: string
  model_source: SettingSource
  models: string[]
  sampled_minutes: number
  max_candidates: number
  /** The lifecycle thresholds: shelved after, abandoned after, staple at, burnt out for. */
  shelve_after_days: number
  abandon_after_days: number
  staple_hours: number
  burnout_days: number
  pattern_half_life_days: number
  pattern_min_games: number
  /** The play diary; switched with PUT /api/taste/diary. */
  play_diary: boolean
  /** The recommended wishlist: off removes it from picks and the app. */
  wishlist: boolean
  galaxy_db: string | null
  galaxy_db_exists: boolean
  /** Whether the OpenAI-compatible model thinks first: auto (off on a home server), off, on. */
  openai_thinking: string
  /** Games hidden in the Steam client are synced too. */
  steam_include_hidden: boolean
  /** Galaxy integrations a sync leaves out (`origin`, `xboxone`). */
  galaxy_skip: string[]
  /** Every store a Galaxy sync has brought games from, GOG's own first. */
  galaxy_integrations: GalaxyIntegration[]
  heroic_dir: string | null
  gog: GogStatus
  epic: EpicStatus
  /** The Epic Games Launcher's manifests folder, when it's on this computer. */
  epic_launcher_dir: string | null
  env_file: boolean
  /** Model used for review digests ("default" = CLI default). */
  digest_model: string
  igdb_client_id: string | null
  igdb_secret_set: boolean
  /** "claude-cli" (Claude subscription, via `claude -p`), "codex-cli" (ChatGPT
   *  subscription, via `codex exec`), "api-key" (Anthropic API), "openai-key"
   *  (OpenAI API) or "openai" (another OpenAI-style server). */
  llm_backend: string
  llm_backends: string[]
  /** Which backend writes review summaries, second chances and hidden gems:
   *  "same" as picks, or one of llm_backends with its own fields. */
  digest_backend: string
  digest_backends: string[]
  /** The Claude models offered for summaries, by the summaries' backend. */
  digest_models: string[]
  anthropic_api_key_set: boolean
  anthropic_api_key_source: SettingSource
  rawg_api_key_set: boolean
  /** "auto" | "on" | "off". */
  linux_mode: string
  linux_modes: string[]
  /** The server runs on Linux. */
  linux_detected: boolean
  /** The server's system: "linux", "windows" or "macos". */
  os: string
  /** Effective value of linux_mode. */
  linux_active: boolean
  /** "arcade" | "ember" | "daylight". */
  theme: ThemeName
  themes: ThemeName[]
  /** Use Daylight when the OS is in light mode. */
  theme_follow_system: boolean
  auto_sync: boolean
  /** Windows only: closing the app window leaves its server running, with the tray icon to open it again. */
  keep_running: boolean
  /** Windows only: the app starts at sign-in with only its tray icon (the Run key). */
  start_with_windows: boolean
  /** Backlog verdicts: the service's address, UpNext's own service unless one is set. */
  verdicts_url: string | null
  verdicts_url_source: SettingSource | null
  /** Offer a backlog verdict after a mark that ends something. */
  verdicts_ask: boolean
  /** Other players' backlog verdicts are downloaded and shown. */
  verdicts_receive: boolean
  verdicts_receive_source: SettingSource
  /** Whether the AI reads them too (picks, "too", Help me decide, Second Chance); on by default since the A/B. */
  verdicts_prompt: boolean
  /** OpenAI-compatible server for llm_backend "openai". */
  openai_base_url: string
  openai_model: string | null
  openai_digest_model: string | null
  openai_api_key_set: boolean
  /** Models for llm_backend "codex-cli": picks, and review summaries. */
  codex_model: string | null
  codex_digest_model: string | null
  /** Reasoning effort; null is Codex's default for the model. */
  codex_effort: string | null
  codex_efforts: string[]
  /** llm_backend "openai-key": the key (never sent back), the two models, the effort. */
  openai_key_set: boolean
  openai_key_source: SettingSource
  openai_key_model: string | null
  openai_key_digest_model: string | null
  /** Null is OpenAI's default for the model. */
  openai_key_effort: string | null
  openai_key_efforts: string[]
  /** The address other devices use, normalized: "http://den.lan:8787". */
  public_url: string | null
  public_url_source: SettingSource
  /** What the server listens on now; other_devices when it's beyond
   *  loopback, https when a certificate is loaded, error when the last
   *  change didn't take, forced when UPNEXT_HOST (a container) decides;
   *  this_computer when the asker is the computer UpNext runs on;
   *  container in UpNext's container image, where nobody is. */
  listen: {
    host: string
    port: number
    other_devices: boolean
    https: boolean
    error: string | null
    forced: boolean
    this_computer: boolean
    container: boolean
  }
  /** "Allow other devices": listen on every address. */
  allow_other_devices: boolean
  /** "Announce on the network": found by phones and other computers without an address. */
  announce: boolean
  /** The computer's name on the network; null is the host name, which `network_name_shown` gives. */
  network_name: string | null
  network_name_shown: string
  /** HTTPS on the same port: a certificate (PEM) and its key, files on the server. */
  tls_cert: string | null
  tls_key: string | null
  /** Signing in with an identity provider: set only on
   *  the computer UpNext runs on, or in its environment. The secret is
   *  write-only; `oidc_redirect_uri` is the address to register at the
   *  provider, null without `public_url`. */
  oidc_issuer: string | null
  oidc_issuer_source: SettingSource
  oidc_client_id: string | null
  oidc_client_id_source: SettingSource
  oidc_client_secret_set: boolean
  oidc_client_secret_source: SettingSource
  oidc_name: string | null
  oidc_name_source: SettingSource
  oidc_auto_provision: boolean
  oidc_auto_provision_source: SettingSource
  oidc_allowed_groups: string | null
  oidc_allowed_groups_source: SettingSource
  oidc_redirect_uri: string | null
  /** Proxies whose forwarding headers are believed, and whether their visitors need no token. */
  trusted_proxies: string | null
  trusted_proxies_source: SettingSource
  trusted_proxy_login: boolean
  trusted_proxy_login_source: SettingSource
}

/** Whether "Sign in with …" is offered on the pairing page. */
export interface OidcStatus {
  enabled: boolean
  name: string | null
}

/** Settings › Devices and API, Add a device (generated from the API's OpenAPI document). */
export type Pairing = components['schemas']['Pairing']
export type PairingOutcome = components['schemas']['PairingOutcome']
export type SteamShortcut = components['schemas']['SteamShortcut']
/** A browser just paired. */
export type Paired = components['schemas']['Paired']
/** Allowing a device with a tap: the request as the
 *  device made it, the computer's answer, and the requests the computer's
 *  dialog shows. */
export type PairAsked = components['schemas']['Asked']
export type PairCollected = components['schemas']['Collected']
export type WaitingDevices = components['schemas']['Waiting']
export type WaitingDevice = components['schemas']['WaitingDevice']
/** Using another computer's UpNext from the app window. */
export type NetworkNearby = components['schemas']['Nearby']
export type FoundComputer = components['schemas']['Found']
export type NetworkConnection = components['schemas']['Connection']

export type TokenScope = 'read' | 'full'

/** An API token as listed: never the token itself. */
export interface ApiToken {
  id: number
  name: string
  scope: TokenScope
  /** "token" (made by hand), "device" (a paired browser) or "oidc" (a browser
   *  signed in with the identity provider). */
  kind: 'token' | 'device' | 'oidc'
  /** The token's first characters: "upnext_Ab12". */
  hint: string
  created_at: number
  last_used_at: number | null
  /** The player it is bound to: it acts for them alone. Null for one that isn't. */
  player_id?: number | null
}

export interface TokensResponse {
  tokens: ApiToken[]
  /** The token this request came with; null from this computer. */
  current: number | null
}

/** A token just made: the only time it's seen in full. */
export interface NewToken extends ApiToken {
  token: string
}

export type Screenshot = components['schemas']['Screenshot']

export type ThemeName = 'arcade' | 'ember' | 'daylight'

export interface LlmTestResult {
  ok: boolean
  backend: string
  model?: string | null
  duration_ms?: number
  usage?: Usage | null
  cost_usd?: number | null
  api_usd?: number | null
  error?: string
}

export interface DecideResult {
  pick_id: number
  reason: string
  model: string | null
  usage: Usage | null
  cost_usd: number | null
  api_usd: number | null
}

/** What an AI call was for. */
export type AiKind = components['schemas']['AiKind']

/** AI calls added up (docs/api/ai-usage.md). */
export interface AiTotals extends Usage {
  calls: number
  /** Of `calls`, the ones that failed. */
  failed: number
  /** Money charged: an API key or a hosted provider. */
  cost_usd: number | null
  /** All of them at list prices, charged or not; what wasn't charged is this less `cost_usd`. */
  api_usd: number | null
  /** What the Claude CLI said its calls would cost on the API. Kept, not shown. */
  would_cost_usd: number | null
  /** Titles or series the calls that worked covered. */
  items: number
  duration_ms: number
}

export interface KindUsage extends AiTotals {
  kind: AiKind
}

export interface ModelUsage extends AiTotals {
  backend: string
  model: string | null
}

export interface DayUsage extends AiTotals {
  /** A local day, "2026-09-30". */
  day: string
  by_kind: KindUsage[]
}

/** `GET /api/ai/usage?days=30`. */
export interface AiUsageSummary {
  days: number
  /** The backend in use now. */
  backend: string
  /** Charged per call (the API key). */
  priced: boolean
  today: AiTotals
  last_7_days: AiTotals
  /** All of the `days`. */
  total: AiTotals
  /** Every day of the period, oldest first, days without calls included. */
  by_day: DayUsage[]
  by_kind: KindUsage[]
  /** Most tokens first. */
  by_model: ModelUsage[]
  /** By who asked: your own calls, and everyone's on the AI accounts you own; null is the library's jobs. */
  by_player?: PlayerUsage[]
  /** By the AI account that ran them: your own accounts first, then the ones shared with you. */
  by_account?: AccountUsage[]
}

/** One AI account's calls in AI usage. */
export interface AccountUsage extends AiTotals {
  /** Null only for calls from before there was any AI account. */
  ai_account_id: number | null
  /** Its owner's name; null once the account is removed. */
  owner: string | null
  mine: boolean
  /** Its backend now, or null for the built-in one or a removed account. */
  backend: string | null
  /** The first account: this computer's own CLIs and keys. */
  this_computer: boolean
}

/** One player's calls in AI usage. */
export interface PlayerUsage extends AiTotals {
  player_id: number | null
  /** Their name, or "The library" for the library's jobs. */
  name: string
}

/** US dollars per million tokens. */
export interface Price {
  input: number
  output: number
  cache_read: number | null
  cache_write_5m: number | null
  cache_write_1h: number | null
}

/** A provider's list prices (`GET /api/ai/prices`). */
export interface PriceInfo {
  provider: 'anthropic' | 'openai' | 'openrouter'
  label: string
  /** The official page, linked wherever a price shows. */
  page_url: string
  /** When they were read; null when the built-in list is in use. */
  fetched_at: number | null
  /** The date the built-in list was checked, when it's in use. */
  built_in_checked: string | null
  /** Why the last read failed, and when. */
  error: string | null
  tried_at: number | null
  models: Record<string, Price>
}

export interface PricesView {
  backend: string
  /** Charged per call, or shown at API prices (a subscription). */
  charged: boolean
  /** Null for a local model, or a provider whose prices UpNext can't read. */
  prices: PriceInfo | null
}

/** One of a plan's limits, read from its CLI. */
export interface PlanWindow {
  /** "session", "week", or "week:Fable" for a model's own weekly limit. */
  id: string
  /** The Claude CLI's "Current week (all models)", or UpNext's "5-hour limit" for the Codex CLI's. */
  label: string
  /** Percent used, never left. */
  used: number
  /** The Claude CLI's own wording: "Oct 4, 3pm (America/New_York)". */
  resets: string | null
  /** Unix seconds, when the CLI gives a number (the Codex CLI does). */
  resets_at: number | null
}

export interface WindowFigure {
  id: string
  label: string
  /** One point of the window, in the measure. */
  per_point: number
  spans: number
  /** Session points per point of this window, when worked out from the session's figure. */
  from_session: number | null
}

export interface PlanShare {
  kind: AiKind
  items: number
  /** Percent of each window, by window id. */
  percent: Record<string, number>
}

/** `GET /api/ai/plan`. */
export interface PlanSummary {
  backend: string
  subscription: boolean
  /** "max", "pro" (Claude; not the tier) or "plus", "pro" (ChatGPT). */
  plan: string | null
  reading: { at: number; windows: PlanWindow[] } | null
  error: string | null
  measured: {
    measure: 'api_usd' | 'tokens'
    spans: number
    windows: WindowFigure[]
    each: PlanShare[]
    one_time: PlanShare | null
  } | null
}

export type SideKind = 'spin-off' | 'remaster' | 'duplicate' | 'expansion' | 'multiplayer' | 'tool' | 'other'

export interface SeriesGuide {
  start_id: number
  /** Main-line play order (owned games only). */
  order: number[]
  side: { id: number; kind: SideKind }[]
  note: string
  model: string | null
  generated_at: number
}

/** A game series with 2+ owned games (GET /api/collections). */
export interface Collection {
  key: string
  name: string
  /** Play order when guided (main line, then side entries), else oldest first. */
  members: GameView[]
  /** Original release year per member id (keys are ids as strings). */
  years: Record<string, number>
  /** Members past untouched and sampled. */
  played: number
  start_id: number | null
  /** Next unplayed main-line game. */
  next_id: number | null
  guide: SeriesGuide | null
  /** Owned games changed since the guide was written. */
  guide_stale: boolean
  /** Smaller series folded into this one ("Black Ops" in "Call of Duty"). */
  subseries: { name: string; ids: number[] }[]
  /** Games in the series you don't own (PC releases, from Wikidata), oldest first. */
  missing: SeriesEntry[]
}

/** A game in a series that isn't in your library. */
export interface SeriesEntry {
  qid: string
  name: string
  year: number | null
  steam_appid: number | null
  /** Cover art found for it (Steam, IGDB or GOG), if any. */
  cover: string | null
  /** The cover lookup has run (so no cover means none was found). */
  cover_checked: boolean
}

/** A store Galaxy brings games from, as the last sync that saw it said. */
export interface GalaxyIntegration {
  /** Galaxy's name, which is the copies' source: `origin`, `xboxone`. */
  platform: string
  label: string
  games: number
  /** When Galaxy last added a game from it (YYYY-MM-DD). */
  last_added: string | null
  /** What Galaxy says; it calls unlinked stores connected too. */
  connected: boolean | null
  left_out: boolean
  /** When a sync last found games from it (Unix seconds). */
  seen_at: number
}

export interface SettingsPatch {
  steam_api_key?: string | null
  steam_id?: string | null
  model?: string | null
  sampled_minutes?: number | string | null
  max_candidates?: number | string | null
  shelve_after_days?: number | string | null
  abandon_after_days?: number | string | null
  staple_hours?: number | string | null
  burnout_days?: number | string | null
  pattern_half_life_days?: number | string | null
  pattern_min_games?: number | string | null
  galaxy_db?: string | null
  galaxy_skip?: string | null
  openai_thinking?: string | null
  steam_include_hidden?: boolean | null
  digest_model?: string | null
  igdb_client_id?: string | null
  igdb_client_secret?: string | null
  llm_backend?: string | null
  digest_backend?: string | null
  anthropic_api_key?: string | null
  rawg_api_key?: string | null
  linux_mode?: string | null
  theme?: ThemeName | null
  theme_follow_system?: boolean | null
  auto_sync?: boolean | null
  keep_running?: boolean | null
  start_with_windows?: boolean | null
  verdicts_url?: string | null
  verdicts_ask?: boolean | null
  verdicts_receive?: boolean | null
  verdicts_prompt?: boolean | null
  openai_base_url?: string | null
  openai_model?: string | null
  openai_digest_model?: string | null
  openai_api_key?: string | null
  codex_model?: string | null
  codex_digest_model?: string | null
  codex_effort?: string | null
  openai_key?: string | null
  openai_key_model?: string | null
  openai_key_digest_model?: string | null
  openai_key_effort?: string | null
  public_url?: string | null
  allow_other_devices?: boolean
  tls_cert?: string | null
  tls_key?: string | null
  wishlist?: boolean | null
  announce?: boolean | null
  network_name?: string | null
  oidc_issuer?: string | null
  oidc_client_id?: string | null
  oidc_client_secret?: string | null
  oidc_name?: string | null
  oidc_auto_provision?: boolean
  oidc_allowed_groups?: string | null
  trusted_proxies?: string | null
  trusted_proxy_login?: boolean
}

export interface SplitResult {
  title: GameView
  detached: GameView
}

export interface SteamTestResult {
  ok: boolean
  persona_name: string | null
  avatar: string | null
  game_count: number
}

/** The next game of a series to play after beating one. */
export interface NextInSeries {
  series: string
  game: GameView
}

export interface LlmModels {
  ok: boolean
  models: string[]
  error?: string
}

/** A model the ChatGPT account can use through Codex. */
export interface CodexModel {
  id: string
  name: string
  description: string
  /** Reasoning efforts it takes, least to most. */
  efforts: string[]
  default_effort: string | null
}

/** GET /api/llm/codex: the Codex CLI, its sign-in and models. */
export interface CodexStatus {
  installed: boolean
  version: string | null
  login: { signed_in: boolean; status: string; per_call: boolean } | null
  /** Why Codex couldn't be run. */
  error: string | null
  /** Families a setting can name instead of a model ("luna"), each with the newest model of it listed today. */
  families: { id: string; model: string }[]
  models: CodexModel[]
  /** "codex" (its own catalog) or "built_in" (UpNext's list, when the catalog can't be read). */
  models_from: 'codex' | 'built_in'
  models_error: string | null
}

/** GET /api/llm/openai: the GPT models an OpenAI API key can use. */
export interface OpenAiKeyStatus {
  key_set: boolean
  /** Families a setting can name instead of a model ("luna"), each with its newest model. */
  families: { id: string; model: string }[]
  /** Newest first. */
  models: string[]
  /** "openai" (the key's own list) or "built_in" (UpNext's, when there's no key or OpenAI can't be reached). */
  models_from: 'openai' | 'built_in'
  models_error: string | null
}

/**
 * Why a backup was made, from its name: the backup job's rotating copies,
 * one made by hand (kept until deleted), or one UpNext made before an
 * upgrade, a restore or an import.
 */
export type BackupKind = 'scheduled' | 'manual' | 'upgrade' | 'restore' | 'import'

/** One backup in the backups folder, from `GET /api/backups`. */
export interface Backup {
  name: string
  kind: BackupKind
  /** When the file was written (Unix seconds). */
  made_at: number
  bytes: number
  /** Its schema version; null when the file isn't a database. */
  schema: number | null
  /** The note given with "Back up now". */
  note?: string
  /** Why UpNext made it on its own: "Before restoring upnext-manual-….db". */
  reason?: string
}

export interface BackupsResponse {
  /** The backups folder on the computer UpNext runs on. */
  folder: string
  /** The newest schema this build reads; a backup above it can't be restored. */
  schema: number
  /** The oldest schema a restore takes, from UpNext's first release; a
   *  backup with a library below it is from a development build. */
  oldest: number
  /** Backups the backup job keeps (Settings › Jobs). */
  keep_scheduled: number
  /** Backups of each automatic kind kept: before an upgrade, a restore or an import. */
  keep_automatic: number
  /** Newest first. */
  backups: Backup[]
}

// ---------------------------------------------------------------------------
// Taste profile (schema v22; docs/api/taste.md)
// ---------------------------------------------------------------------------

/** A reason, or a choice of one: "aspect:story" read as "the story". */
export interface TasteChoice {
  key: string
  label: string
}

export interface TasteFavourite {
  id: number
  game: GameView
  why: TasteChoice[]
  /** What else the game offers as a reason. */
  choices: TasteChoice[]
  note: string | null
  updated_at: number
}

export type StatementKind = 'like' | 'dislike' | 'hard_no'

export interface TasteStatement {
  id: number
  kind: StatementKind
  /** "facet:grind:heavy", "tag:Horror", "length:short" or "text:…". */
  about: string
  label: string
  note: string | null
  /** Games in the library it's about; null for your own words. */
  matches: number | null
  updated_at: number
}

export type HabitKey = 'weeknight' | 'weekend' | 'times' | 'who' | 'where'

export interface TasteHabit {
  id: number
  about: HabitKey
  /** Minutes (weeknight, weekend) or a list of choices. */
  value: number | string[] | null
  note: string | null
  updated_at: number
}

export type TasteOutcome =
  | 'favourite'
  | 'staple'
  | 'beaten'
  | 'played_through'
  | 'still_playing'
  | 'picked'
  | 'shelved'
  | 'abandoned'
  | 'not_for_me'
  | 'turned_down'

export interface EvidenceGame {
  id: number
  name: string
  outcome: TasteOutcome
  at: number | null
  weight: number
  /** Game reasons given for putting it down (stop-reason codes). */
  reasons: string[]
}

export interface TastePattern {
  id: number
  key: string
  sentence: string
  sense: 'stick' | 'drift' | null
  summary: string
  confidence: number
  sure: boolean
  games: EvidenceGame[]
  hidden: boolean
  since: number
}

export interface TasteSubject {
  about: string
  label: string
  group: string
  count: number
}

export interface TasteResponse {
  favourites: TasteFavourite[]
  statements: TasteStatement[]
  habits: TasteHabit[]
  patterns: TastePattern[]
  hidden_patterns: TastePattern[]
  updated_at: number | null
  judged: { kept: number; left: number }
  suggestions: GameView[]
  subjects: TasteSubject[]
  /** What the play diary shows about your habits; empty fields when it holds too little. */
  diary: DiaryHabits
  task: { schedule: string; enabled: boolean; last_run: Job | null; next_run_at: number | null }
  rules: { half_life_days: number; min_games: number }
}

/** A row of the profile as stored (what statement calls answer with). */
export interface TasteRow {
  id: number
  kind: StatementKind | 'favourite' | 'context' | 'pattern'
  title_id: number | null
  about: string | null
  why: unknown
  note: string | null
  source: 'stated' | 'derived'
  hidden: boolean
}

export type TasteSourceId = 'library' | 'statuses' | 'reactions' | 'answers' | 'diary' | 'wishlist' | 'ratings'

export interface TasteSources {
  library: { games: number; played: number; hours: number; stores: Source[]; last_sync: number | null }
  statuses: { games: number; by: Partial<Record<OverrideStatus, number>> }
  reactions: {
    picks: Partial<Record<FeedbackAction, number>>
    /** Picks you rated, by verdict. */
    pick_ratings: Partial<Record<PickVerdict, number>>
    stop_reasons: number
    second_chance_answers: number
  }
  answers: { favourites: number; likes: number; dislikes: number; hard_nos: number; habits: number; routines: number; not_really: number }
  diary: DiaryState
  wishlist: { on: boolean; steam: number; added: number; suggested: number; answered: number; bought: number }
  /** Your ratings of games, by stars "1" to "5". */
  ratings: { games: number; by: Partial<Record<string, number>> }
}

/** One of your ratings of a game, for the Profile's "See it". */
export interface GameRatingRow {
  title_id: number
  name: string
  stars: number
  at: number
}

/** The play diary's state (schema v23). */
export interface DiaryState {
  on: boolean
  /** Hours with play recorded. */
  rows: number
  days: number
  launches: number
  since: number | null
  last_reading: number | null
  /** The last reading's error, when it failed. */
  error: string | null
  /** Why it's on but not reading. */
  paused: string | null
}

export interface DiarySessions {
  minutes: number
  count: number
}

export interface DiaryHabits {
  days: number
  minutes: number
  weeknight: DiarySessions | null
  weekend: DiarySessions | null
  /** The `times` habit's choices with a fair share of your play, most first. */
  times: string[]
  multiplayer: { minutes: number; weekend_share: number; time: string | null } | null
}

/** A row of the play diary: Steam minutes in an hour, or a launch from the Play button. */
export interface DiaryRow {
  id: number
  hour: number
  at: number
  title_id: number | null
  appid: number | null
  name: string
  minutes: number
  source: 'steam' | 'launch'
}

/** The first-run questions, GET /api/taste/questions. */
export interface TasteQuestions {
  most_played: { game: GameView; favourite: number | null }[]
  /** Too little playtime for covers: search instead. */
  few_played: boolean
  loves: TasteSubject[]
  hard_nos: TasteSubject[]
  diary: { on: boolean; steam: boolean }
}

/** What a reaction was: a pick card's action or rating, why you stopped, or a Second Chance answer. */
export type ReactionWhat = FeedbackAction | 'good_pick' | 'not_a_fit' | 'reasons' | 'second_chance'

export interface ReactionRow {
  at: number
  title_id: number
  name: string
  /** A pick action, a pick's rating, "reasons" or "second_chance". */
  what: ReactionWhat
  text: string | null
  note: string | null
}

export interface StatementBody {
  kind: StatementKind | 'favourite' | 'context'
  title_id?: number
  about?: string
  why?: unknown
  note?: string | null
}

export interface StatementEdit {
  kind?: StatementKind
  why?: unknown
  note?: string
  hidden?: boolean
  confirm?: boolean
}

/** UpNext's shared game data: what is applied, and the data server's address. */
export type SeedStatus = components['schemas']['SeedStatus']
/** What applying shared data did. */
export type SeedApplied = components['schemas']['SeedApplied']
/** Published shared data, read before downloading it: how many games and how recent. */
export type SeedManifest = components['schemas']['SeedManifest']

// ---------------------------------------------------------------------------
// Backlog verdicts: your own, sent to the verdicts service
// ---------------------------------------------------------------------------

/** Where one of your verdicts stands. */
export type VerdictState = 'draft' | 'queued' | 'sent' | 'held' | 'withdrawn' | 'removed'

/** Your answers in the verdict form: codes from the contract's fields. */
export interface VerdictAnswers {
  outcome?: string | null
  worth?: string | null
  hours?: string | null
  clicked?: string | null
  sessions?: string | null
  limited_time?: string | null
  factors?: string[]
  stop_point?: string | null
  stop_note?: string | null
  stopped_for?: string[]
  text?: string | null
}

/** What's wrong with one answer, as the form shows it beside the field. */
export interface VerdictProblem {
  field: string
  why: string
}

/** One of your verdicts as this computer keeps it. */
export interface MyVerdict {
  title_id: number
  name: string
  answers: VerdictAnswers
  steam_appid: number | null
  gog_id: string | null
  remote_id: string | null
  state: VerdictState
  retry_at: number | null
  error: string | null
  sent_at: number | null
  updated_at: number
}

/** The consent screen, and whether you accepted its current version. */
export interface VerdictConsent {
  version: number
  title: string
  paragraphs: string[]
  accept: string
  decline: string
  text_box_line: string
  licence: string
  licence_url: string
  terms_url: string | null
  accepted: number | null
  accepted_on: string | null
  current: boolean
}

/** Where a pre-filled answer came from. */
export type VerdictSource = 'mark' | 'playing' | 'playtime' | 'diary' | 'stop_reasons' | 'no_ending'

/** GET /api/v1/verdicts/draft/{id}: the verdict form for a game. */
export interface VerdictForm {
  title_id: number
  name: string
  cant_send: string | null
  answers: VerdictAnswers
  from: Partial<Record<string, VerdictSource>>
  problems: VerdictProblem[]
  mine: MyVerdict | null
  outdated: boolean
  service: boolean
  consent: VerdictConsent
}

/** GET /api/v1/verdicts: your verdicts and what sending needs. */
export interface VerdictsOverview {
  service: boolean
  has_key: boolean
  consent: VerdictConsent
  verdicts: MyVerdict[]
}

/** What a send ended as: sent, held, or queued to try again. */
export interface VerdictSent {
  verdict: MyVerdict
  message: string
}

/** One question of the verdict form, from the contract. */
export interface VerdictField {
  name: string
  question: string
  required: boolean
  many?: boolean
  text?: boolean
  max_chars?: number
  /** Shown only with an answer to this field. */
  with?: string
  /** Shown only for these outcomes. */
  only_for?: string[]
  choices?: { code: string; label: string; helps?: boolean }[]
}

/** GET /api/v1/verdicts/vocab: the contract the form is built from. */
export interface VerdictContract {
  fields: VerdictField[]
  limits: { text_chars: number; stop_note_chars: number; factors: number }
  text_checks: { code: string; why: string; pattern: string; flags: string }[]
  text_ignored: { pattern: string; flags: string }
  report_reasons: { code: string; label: string }[]
}

// ---------------------------------------------------------------------------
// Backlog verdicts: other players', from the daily snapshot
// ---------------------------------------------------------------------------

/** A game's verdicts as the library carries them: the headline, the "most players" lines and the majorities. */
export type Backlog = components['schemas']['Backlog']
/** The snapshot this install keeps. */
export type CommunitySnapshot = components['schemas']['CommunitySnapshot']
/** Whether other players' verdicts are on, and the snapshot kept. */
export type CommunityStatus = components['schemas']['CommunityStatus']
/** A game's verdicts as its page shows them. */
export type GameVerdicts = components['schemas']['GameVerdicts']
/** The game page's section: the status, and the game's verdicts when it has at least 3. */
export type GameCommunity = components['schemas']['GameCommunity']
/** A text from another player, with its verdict id for reporting it. */
export type ShownTextView = components['schemas']['ShownTextView']

// ---------------------------------------------------------------------------
// Players: who plays here, their accounts and what they hid
// ---------------------------------------------------------------------------

/** A player as the API shows them. */
export type Player = components['schemas']['PlayerView']

/** `GET /api/players`: everyone, and who this browser or token is playing as. */
export interface PlayersResponse {
  players: Player[]
  /** Null while the default player has a PIN and nobody has been chosen here. */
  current: number | null
  /** The token this came with is bound to `current`: it can't switch. */
  bound: boolean
}

/** "Hide this game": from you, from everyone else, or both. */
export type HideScope = components['schemas']['HideScope']

/** A game you hid, on the Lists page's Hidden by me. */
export interface HiddenTitle {
  title_id: number
  name: string
  scope: HideScope
  at: number
  /** A copy in an account of your own, which hiding it from the others needs. */
  own_copy?: boolean
}

/** Who an account is offered to: everyone, the players its owner ticks, or only its owner. */
export type Sharing = 'everyone' | 'chosen' | 'owner'

/** Another player's part in an account, as its owner sees it. */
export type AccountPlayer = components['schemas']['AccountPlayer']

/** A store account as a player sees it in Settings. */
export type StoreAccount = Omit<components['schemas']['StoreAccount'], 'sharing'> & { sharing: Sharing }

/** A store connection, for its owner. */
export type StoreConnection = components['schemas']['StoreConnection']

/** `GET /api/accounts`. */
export interface AccountsResponse {
  connections: StoreConnection[]
  accounts: StoreAccount[]
}

/** An AI account as a player sees it: never its keys. */
export type AiAccount = Omit<components['schemas']['AiAccount'], 'sharing' | 'settings'> & {
  sharing: Sharing
  settings: Record<string, string>
}

/** What hiding a game answers: hidden from the others, who still sees it,
 *  and which of them only through another player's shared account. */
export type HideAnswer = components['schemas']['HideAnswer']
