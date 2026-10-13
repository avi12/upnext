# Sync and enrich sources

[API index](../API.md) · [Jobs](jobs.md) · [Scheduled jobs](schedules.md) · [Game details](details.md)

## GET /api/sync/sources

The libraries a sync reads, in order: `[{"id": "steam", "label": "Steam", "blocked": null}, {"id": "gog", "label": "GOG account", "blocked": "not connected"}]`. The ids are `steam`, `heroic`, `gog` (blocked without a GOG sign-in), `epic` (blocked without an Epic sign-in or the Epic Games Launcher), `ea` and `ubisoft` (blocked when nothing they installed is found; Windows only) and `galaxy` (blocked without `galaxy_db`).

## Enrich options

One boolean per source, keyed by its id, plus `refresh` (fetch again what titles have already), `limit` (at most this many games) and `titles` (a list of title ids to limit the run to). A source left out takes its default, and an unknown key is ignored. Enrich always runs the Steam-store name match for non-Steam games.

`GET /api/enrich/sources` lists them in run order: `[{"id": "hltb", "label": "HowLongToBeat", "default": false, "note": "slow", "blocked": null}]`. `note` is a word on cost ("slow", "uses the AI"), and `blocked` says why the settings keep a source from running ("needs a key", "needs an IGDB login", "Linux mode is off"). The ids are `wikidata`, `series`, `art`, `reviews`, `review_texts`, `tags`, `protondb`, `gog_meta`, `igdb`, `rawg`, `pcgw`, `hltb` (off by default), `trailers`, `digests` (off), `series_guides` (off), `second_chances` (off), `hidden_gems` (off) and `wishlist` (off; its own task).

Sources that take every title run in parallel lanes, one per rate-limited service ([Jobs](jobs.md#the-job)). Steam cover art is fetched before the lanes start and again after them. Titles go in order: pick candidates first, then installed games, then the most-reviewed. The AI passes (review summaries, series guides) run three calls at a time.
