# Series

[API index](../API.md) · [Game details](details.md) · [Wishlist](wishlist.md)

`GameView.series` lists the series a title belongs to, from Wikidata (series membership) and PCGamingWiki, matched by a normalised name ("The Witcher" is "Witcher", HTML entities decoded). `GameView.first_year` is the original release year from Wikidata, or null.

Two enrich sources fill them in: `series` (on by default) for every title with a Wikidata item, and `series_guides` (off by default; uses the AI with the picks' `model`), which writes a guide for each series with two or more owned games. A series item is found by name when a label came without a Wikidata id, as PCGamingWiki's do.

## GET /api/collections

Every series with at least two games, owned or not (not-games left out), started series first, then larger ones:

```json
[{
  "key": "witcher",
  "name": "The Witcher",
  "members": ["GameView", "..."],
  "years": {"412": 2007, "413": 2011},
  "played": 2,
  "start_id": 412,
  "next_id": 412,
  "guide": {"start_id": 412, "order": [412, 413, 414], "side": [{"id": 500, "kind": "spin-off"}],
            "note": "Start with Witcher 1 ...", "model": "claude-sonnet-5-5", "generated_at": 1790575000},
  "guide_stale": false,
  "missing": [{"qid": "Q…", "name": "…", "year": 2015, "steam_appid": 292030,
               "igdb_slug": "…", "gog_id": "…", "gog_slug": "…", "cover": "https://…"}],
  "subseries": [{"name": "Call of Duty: Black Ops", "ids": [101, 102]}]
}]
```

- `members` are in play order when there's a guide (main line first, then side entries), else oldest first.
- `played` counts the members that aren't untouched or sampled.
- `start_id` is where to begin (the guide's, else the first in order). `next_id` is the next unplayed main-line game, or the start when that's unplayed.
- `guide` is null until one is written. It names the game to start with, the main-line order, and the owned entries outside it with their kind (`spin-off`, `remaster`, `duplicate`, `expansion`, `multiplayer`, `tool` or `other`), plus a note of a sentence or two. It's written once per series and again only when the owned games change, which `guide_stale` flags. The guide orders the main line by release or story, never by quality, and reads the unowned entries for context.
- `missing` are the series' games you don't own, oldest first, from Wikidata: PC releases only (Windows, or on Steam, GOG or Epic). Entries matching a game you own (by Wikidata id, Steam appid, or name including an edition) are left out, and an owned game that matches but had no series tag joins the members instead. `igdb_slug`, `gog_id` and `gog_slug` are there when Wikidata has them, and `cover` when the series pass found one.
- `subseries` are smaller series folded into this one.

## GET /api/games/{id}/series

The collection holding that title, or null. The game sheet's "In this series" uses it rather than downloading every collection.

## GET /api/games/{id}/next-in-series

`{"series": "Metro", "game": GameView}`, or null. The game is the first unplayed main-line entry after `{id}` in play order, else the series' own next pick. It's null when that game is on Play later already. The UI asks for it after a game is marked `done` and offers "Save for later".
