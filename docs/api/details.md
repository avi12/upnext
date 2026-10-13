# Game details

[API index](../API.md) · [Games](games.md) · [Review tags](review-tags.md)

Fetching details (the `details` and `reviews` [scheduled tasks](schedules.md)) fills in these `GameView` fields. Each is null or empty until its source has run.

## Tags, themes and ratings

```json
{
  "tags": ["Story Rich", "Atmospheric", "Stealth", "..."],
  "tags_by_source": {"steam": ["..."], "reviews": ["Relaxing", "Short sessions"], "gog": ["..."], "igdb": ["..."]},
  "themes": ["Horror", "Third person", "Single player"],
  "gog_rating": 4.5, "gog_rating_count": 1200,
  "igdb_rating": 81.3, "igdb_rating_count": 240,
  "rawg_rating": 4.2, "rawg_ratings_count": 900,
  "rawg_url": "https://rawg.io/games/...", "pcgw_url": "https://www.pcgamingwiki.com/wiki/...",
  "wikidata_url": "https://www.wikidata.org/wiki/Q..."
}
```

- `tags` is one de-duplicated list from every source, in this order: SteamSpy's user tags, the review tags' plain labels, GOG's user tags, RAWG, IGDB (keywords, themes and genres) and PCGamingWiki. At most 30.
- `tags_by_source` keeps them apart (`steam`, `reviews`, `gog`, `rawg`, `igdb`, `pcgw`), on the game page only. `reviews` holds the plain labels of a game's moods, scales and skills ("Relaxing", "Easy", "Short sessions"), so they can be searched; aspects and notes aren't there.
- `themes` are IGDB's themes, perspectives and game modes, and PCGamingWiki's themes, perspectives, pacing, modes and art styles. `categories` include GOG's features.
- `genres` and `developers` fall back to RAWG, then PCGamingWiki, when the stores have none, and `metacritic` falls back to RAWG. RAWG's and GOG's ratings stand in for the review score in picks when Steam has none.
- `rawg_url`, `pcgw_url` and `wikidata_url` are the source pages. RAWG's free plan asks for a visible link back wherever its data is shown, so a game page with `rawg_url` shows a "RAWG" link to it, and anything that filters by RAWG's tags credits RAWG.

## What players wrote

```json
"review_texts": [
  {"source": "steam", "positive": true, "rating": null, "votes": 61, "hours": 11, "title": null, "posted": 1758000000, "text": "I enjoyed this quite a bit..."},
  {"source": "gog", "positive": null, "rating": 5, "votes": 12, "hours": null, "title": "Masterpiece", "posted": 1757000000, "text": "..."}
],
"digest": "A stealth game whose story and atmosphere carry it; the stealth itself is simple and the ending divides people.",
"vibes": "You creep through rain-slick streets..."
```

- `review_texts` are whole. Steam gives up to 10 English reviews (Steam caps them at 8,000 characters): about 6 of the all-time most helpful, then the most helpful of the last month. GOG gives up to 8 of its most voted. Steam reviews have a thumb (`positive`), votes and hours played; GOG's have 1 to 5 stars (`rating`) and a title. `posted` is in Unix seconds, or null when it isn't known.
- `digest` (People say) is the AI's critical look in at most 40 words: what sets the game apart, what it does well, and where it falls short.
- `vibes` is what playing it feels like, in 3 or 4 sentences in the reviewers' own images; null when the reviews say nothing about the experience.
- The same AI call writes `digest`, `vibes` and [`review_tags`](review-tags.md), from the same reviews, as the summaries job (`digests`) does. That job writes again any summary from an older prompt, or one that came back empty or as a stand-in word.

### POST /api/games/{id}/reviews

Fetches the title's player reviews now (Steam's by appid, with its score, else GOG's for its GOG copy), stores them as the enrich sources do, and answers with the `GameView`. The game page calls it when "What players wrote" is opened on a title whose summary came from [shared data](seed.md), which has no reviews stored. `400` when the title has neither store; `404` for no such title; `502` when the store doesn't answer.

## Where the details come from

- **Steam store, reviews and SteamSpy**: details, review scores, review texts and user tags. Non-Steam games are matched by name against Steam's store first.
- **Wikidata** (no key): cross-references, looked up in batches by Steam appid, then by exact title or alias (ambiguous titles are skipped). It gives each title its PCGamingWiki page, RAWG slug, HowLongToBeat id and Epic slug, so the other sources match by id, plus its series and first release year ([Series](collections.md)).
- **RAWG**, only with a RAWG key: tags, genres, rating and Metacritic, by Wikidata's slug or else a name search that has to match closely.
- **PCGamingWiki** (no key): genres, themes, perspectives, pacing, modes, art styles and series.
- **GOG**: ratings, reviews, user tags and features. **IGDB**, only with IGDB credentials: rating, keywords, themes, modes and trailers.
- **ProtonDB** ([Linux](#linux)) and **HowLongToBeat** (off unless turned on).

## Linux

These run only while Linux compatibility is active. `linux_mode` is `auto` (on when the server runs on Linux), `on` or `off`, as `linux_modes` lists them, and `GET /api/settings` also gives `linux_detected` (the server runs on Linux) and `linux_active` (the result):

- `linux_native`: Steam lists a Linux build (or null).
- `protondb_tier`, `protondb_trending` and `protondb_total` (the report count): ProtonDB's view, skipped for games with a native build.
- `linux`: `native`, else the ProtonDB tier (`platinum`, `gold`, `silver`, `bronze`, `borked` or `pending`), or null when unknown. "Runs well" is native, platinum or gold.

Picks then favour games that run well, push bronze and borked down, and tell the AI to avoid silver, bronze and borked unless asked.
