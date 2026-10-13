/**
 * Where UpNext's game data comes from, for Settings › About. A source whose
 * terms ask for attribution says so in `note`, with the licence or terms
 * linked; docs/THIRD_PARTY.md has every service's terms. Keep the two in
 * step when a source is added.
 */
export type Credit = {
  name: string
  url: string
  /** What UpNext shows from it. */
  what: string
  /** The attribution its terms ask for, or other terms worth a line. */
  note?: string
  terms?: { label: string; url: string }
}

export const CREDITS: Credit[] = [
  {
    name: 'Steam',
    url: 'https://store.steampowered.com',
    what: 'Your Steam library, store details, review scores, player reviews and art',
    note: 'Data from the Steam Web API and store. UpNext is not affiliated with Valve.',
    terms: { label: 'Steam Web API Terms of Use', url: 'https://steamcommunity.com/dev/apiterms' },
  },
  { name: 'SteamSpy', url: 'https://steamspy.com', what: 'Player tags' },
  {
    name: 'PCGamingWiki',
    url: 'https://www.pcgamingwiki.com',
    what: 'Genres, themes, perspectives, pacing and series',
    note: 'PCGamingWiki content is available under CC BY-NC-SA 3.0.',
    terms: { label: 'CC BY-NC-SA 3.0', url: 'https://creativecommons.org/licenses/by-nc-sa/3.0/' },
  },
  {
    name: 'RAWG',
    url: 'https://rawg.io',
    what: 'Tags, genres and ratings, with your own key',
    note: 'Source of this data: RAWG.',
    terms: { label: 'RAWG API terms', url: 'https://rawg.io/apidocs' },
  },
  {
    name: 'IGDB',
    url: 'https://www.igdb.com',
    what: 'Genres, themes, keywords and trailers, with your own Twitch app',
    terms: { label: 'Twitch Developer Services Agreement', url: 'https://legal.twitch.com/en/legal/developer-agreement/' },
  },
  {
    name: 'Wikidata',
    url: 'https://www.wikidata.org',
    what: 'Links between stores and databases, and series',
    note: 'Wikidata data is CC0.',
    terms: { label: 'CC0 1.0', url: 'https://creativecommons.org/publicdomain/zero/1.0/' },
  },
  { name: 'ProtonDB', url: 'https://www.protondb.com', what: 'How well games run on Linux through Proton' },
  { name: 'GOG', url: 'https://www.gog.com', what: 'Your GOG library and playtime, store tags and reviews' },
  { name: 'Epic Games', url: 'https://store.epicgames.com', what: 'Your Epic library and playtime, when you sign in' },
  { name: 'HowLongToBeat', url: 'https://howlongtobeat.com', what: 'How long games take, only when you turn it on' },
]
