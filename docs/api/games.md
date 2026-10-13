# Games: the GameView

[API index](../API.md) · [The library](library.md) · [Titles and copies](titles.md)

## Sources and launchers

Games come from several libraries and are keyed by a stable internal integer `id`, not the Steam appid:

| `source` | `launcher` | `source_id` | `run_url` | Playtime known |
|---|---|---|---|---|
| steam | steam | Steam appid as a string | `steam://run/{appid}` | yes |
| epic | heroic | Epic app_name | `heroic://launch/legendary/{app_name}` | no |
| gog | heroic | GOG product id | `heroic://launch/gog/{id}` | no |
| amazon | heroic | Amazon product id | `heroic://launch/nile/{id}` | no |
| gog, steam, epic… | galaxy | Galaxy release key | `goggalaxy://openGameView/{releaseKey}` | yes (Galaxy) |
| epic | epic | Epic app_name | `com.epicgames.launcher://apps/…?action=launch&silent=true` | yes (Epic account) |
| origin | origin | EA content id | none | no |
| uplay | uplay | Ubisoft game id | `uplay://launch/{id}/0` | no |
| origin, uplay, battlenet, xboxone | manual | the title's key | none | no |

Fetching details matches non-Steam games by name against the Steam store, so they still get genres, tags and reviews; `appid` is then the matched appid and `steam_matched` is true. With no match, `appid` is null and only the launcher's own details (art, description, genres) are there. The same `(source, source_id)` seen from two launchers (Steam natively and through Galaxy, say) is one row, and the native launcher wins. [More stores](more-stores.md) has Epic, the EA app, Ubisoft Connect and games added by hand.

## The GameView

A game in every answer is a `GameView`. Each is a title, which groups a game's copies across stores ([Titles and copies](titles.md)). The core of it:

```json
{
  "id": 1234, "source": "steam", "source_id": "620", "launcher": "steam",
  "appid": 620, "steam_matched": false, "installed": true, "name": "Portal 2",
  "playtime_minutes": 0, "playtime_known": true, "playtime_2weeks": 0, "last_played": 0,
  "status": "untouched", "override": null, "override_note": null,
  "kind": "ending", "no_ending": null, "staple": false,
  "shelved_at": null, "abandoned_at": null, "burnt_out_until": null, "stop_reasons": null,
  "type": "game", "short_description": "The Portal 2 ...",
  "genres": ["Action", "Adventure"], "categories": ["Single-player", "Co-op"],
  "tags": ["Puzzle", "Co-op", "First-Person"],
  "header_image": "https://cdn.cloudflare.steamstatic.com/steam/apps/620/header.jpg", "art_square": null,
  "metacritic": 95, "release_date": "18 Apr, 2011", "developers": ["Valve"],
  "review_score": 9, "review_desc": "Overwhelmingly Positive",
  "review_positive": 290000, "review_total": 295000, "review_pct": 98,
  "hltb_main": 8.5, "hltb_extra": 13.0, "hltb_complete": 20.5, "hltb_url": "https://howlongtobeat.com/game/7231",
  "enriched": true, "store_url": "https://store.steampowered.com/app/620", "run_url": "steam://run/620"
}
```

- `id` is the title id, used in every URL and in picks. `appid` is null for a non-Steam game with no store match.
- `playtime_known` is false for games from Heroic, which doesn't track playtime; they stay `untouched` until marked. `playtime_minutes` is the most any copy that tracks it reports.
- `header_image` is the Steam header for Steam games and matched games, otherwise the launcher's cover art, and is never null. `art_square` is portrait or square art when the launcher has it.
- `store_url` is the store page of the preferred copy (Steam, Epic, GOG or Amazon).
- `status` and `override` are explained in [Statuses](statuses.md), with `kind`, `no_ending`, `staple`, `shelved_at`, `abandoned_at`, `burnt_out_until`, `stop_reasons`, `override_at`, `rejections`, `abandoned_by_picks`, `too` and `chosen`.
- `review_score` is Steam's 1 to 9 bucket, `review_desc` its label ("Very Positive"), `review_pct` the positive percentage. `hltb_*` are hours.
- Nullable: `appid`, `art_square`, `type`, `short_description`, `metacritic`, `release_date`, `review_*`, `hltb_*`, `override`, `override_note`. `genres`, `categories` and `developers` may be empty.
- `controller_support` is Steam's word for how well a gamepad works with it, `full` or `partial`, from the store details; null when Steam doesn't say, which mostly means a mouse and keyboard.

Other fields belong to features with pages of their own:

| Fields | Page |
|---|---|
| `copies`, `owned_on`, `primary_source`, `editions` | [Titles and copies](titles.md) |
| `tags`, `tags_by_source`, `themes`, `review_texts`, `digest`, `vibes`, ratings from GOG, IGDB and RAWG, `linux`, source pages | [Game details](details.md) |
| `review_tags` | [Review tags](review-tags.md) |
| `series`, `first_year` | [Series](collections.md) |
| `saved_at`, `trailer_url` | [The library](library.md) |
| `my_rating`, `rated_at`, `pick_rating` | [Ratings](ratings.md) |
| `second_chance`, `second_try` | [Second chances](second-chances.md) |
| `seed_date` | [Shared game data](seed.md) |
| `backlog` | [Backlog verdicts from others](community-verdicts.md) |
| `hidden_scope`, `own_copy` | [Hiding games](hiding.md) |
| `from_wishlist` (game page only) | [Wishlist](wishlist.md) |

Lists carry a slimmer `GameView` than `GET /api/games/{id}`: only the game page has `tags_by_source`, for example.
