// The tour's stops. Each points at a `data-tour`
// attribute in the UI, never at a class, so a restyle can't drop a stop
// quietly; stops.test.ts checks that every id here is in the source.

/** What a stop's text can depend on, read from the page as the tour runs. */
export interface StopContext {
  /** The lineup section's `data-state`: `loading` (the last lineup is on
   *  its way), `empty` (nothing curated yet), `rolling`, `error` or `filled`. */
  lineup: string | null
  /** An AI is set up, so Curate asks it. */
  ai: boolean
  /** The lineup is the tour's own example, ranked by UpNext alone. */
  example: boolean
  /** The sheet stop found a game to open. */
  sheetGame: boolean
}

export interface TourStop {
  id: string
  /** The page the stop is on; the tour goes there itself. */
  path: string
  /** The `data-tour` id of what the stop highlights. */
  target: string
  /** Highlighted instead when the sheet stop has no game to open. */
  fallback?: string
  /** A list (`data-tour` id) inside the target whose first item is
   *  highlighted instead when the whole target leaves the card no room
   *  beside it, so the card doesn't cover what it talks about. */
  firstOf?: string
  /** A disclosure (`data-tour` id) the tour opens first when it's collapsed. */
  open?: string
  /** The stop opens the game sheet on a well-reviewed game with summaries. */
  sheet?: boolean
  /** With no lineup yet, the stop fills one as an example: picks ranked by
   *  UpNext alone, at once and with no AI call. */
  example?: boolean
  title: string
  body: (c: StopContext) => string
}

export const TOUR_STOPS: TourStop[] = [
  {
    id: 'ask',
    path: '/',
    target: 'ask',
    title: "Say what you're up for",
    body: () =>
      "Type it your own way: how long you have, a mood, a genre. Curate picks from the games you own and haven't finished.",
  },
  {
    id: 'lineup',
    path: '/',
    target: 'lineup',
    firstOf: 'picks',
    example: true,
    title: 'Your lineup',
    body: ({ lineup, ai, example }) =>
      lineup === 'filled' && example
        ? `Three from your library as an example, ranked by UpNext alone. ${
            ai ? 'Curate asks the AI, and then each pick says why it fits.' : 'With an AI set up, each pick also says why it fits.'
          } Curate again (or R) brings others, and Compare on two picks shows them side by side.`
        : lineup === 'filled'
          ? 'Each pick says why it fits, then Vibes and People say sum up what players write. Curate again (or R) brings others, and “Not these, because…” tells the next lineup what to avoid. Tick Compare on two picks to see them side by side in Versus.'
          : lineup === 'rolling'
            ? 'Curating… The AI can take a minute. Next carries on without waiting.'
            : lineup === 'error'
              ? "Curating didn't work; the reason is on the page. Next carries on."
              : 'Your picks show up here, each with why it fits, what players say, and ways to curate again or compare two.',
  },
  {
    id: 'sort',
    path: '/library',
    target: 'sort',
    title: 'Every underlined word is a menu',
    body: () =>
      'The sentence at the top of the library is its filter: click any underlined word to change it. This one sorts, by review score, name, story length, release, play time, last played or Metacritic.',
  },
  {
    id: 'more-filters',
    path: '/library',
    target: 'more-filters',
    open: 'more-filters-toggle',
    title: 'More filters',
    body: () =>
      "Genre, tag and installed, and what reviews say: difficulty, mood, session length, what players praise and what they don't complain about. Reviews don't cover every game, so a note says how many could match.",
  },
  {
    id: 'views',
    path: '/library',
    target: 'views',
    title: 'Covers or a table',
    body: () =>
      "Covers are for choosing; the table is the whole collection. In the table, click a column's header to sort by it, and again to turn the order around.",
  },
  {
    id: 'sheet',
    path: '/library',
    target: 'sheet',
    fallback: 'games',
    sheet: true,
    title: 'Every game has a sheet',
    body: ({ sheetGame }) =>
      sheetGame
        ? "Click a game to open it. People say and Vibes sum up its reviews, How it plays and Tags say what it's like, and “Mark as…” records how it went. Save for later puts it on your Play later list."
        : 'Click a game to open its sheet: what players say, how it plays, “Mark as…” to record how it went, and Save for later.',
  },
  {
    id: 'collections',
    path: '/collections',
    target: 'collections',
    title: 'Series, in order',
    body: () =>
      "Collections has every series you own two or more games from, in play or release order, with where to start and what's next. The underlined words choose which series show.",
  },
  {
    id: 'lists',
    path: '/lists',
    target: 'lists',
    title: 'Lists',
    body: () =>
      "Play later holds what you saved for later. Next to it are Hidden gems, your staples, and a list for every way you've marked a game.",
  },
  {
    id: 'history',
    path: '/history',
    target: 'history',
    title: 'History',
    body: () =>
      "Every lineup UpNext has curated for you, with what you did with each pick. The underlined words sort and filter them, and the box finds a game or a request.",
  },
  {
    id: 'keys',
    path: '/',
    target: 'topbar',
    title: 'Keys, and finding this again',
    body: () =>
      'On a keyboard, / searches your library from any page and 1 to 5 switch tabs. Settings › Setup has Show me around, to take this tour again, and Restart the setup wizard.',
  },
]

/** Every `data-tour` id the tour looks for. */
export function tourAnchors(): string[] {
  const ids = TOUR_STOPS.flatMap((s) => [s.target, s.fallback, s.firstOf, s.open]).filter((x): x is string => !!x)
  return [...new Set(ids)]
}
