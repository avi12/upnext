# Titles and copies

[API index](../API.md) · [Games](games.md) · [The library](library.md)

A game owned on several stores is one `GameView`, a title. Each store's copy is listed under `copies`:

```json
{
  "id": 512, "name": "A Short Hike", "appid": 1055540, "steam_matched": true,
  "owned_on": ["gog", "epic"], "primary_source": "gog", "editions": [],
  "copies": [
    {"game_id": 646, "source": "gog", "source_id": "1901367087", "launcher": "heroic", "installed": true,
     "name": "A Short Hike", "edition": null,
     "playtime_minutes": 0, "playtime_known": false, "last_played": 0,
     "run_url": "heroic://launch/gog/1901367087", "store_url": "https://www.gog.com/..."},
    {"game_id": 5, "source": "epic", "source_id": "d640...", "launcher": "heroic", "installed": false, "...": "..."}
  ],
  "source": "gog", "source_id": "1901367087", "launcher": "heroic", "run_url": "heroic://launch/gog/1901367087", "...": "..."
}
```

- `id` is the title id, and stays the same. `copies[].game_id` is the store's own row.
- `owned_on` lists the stores in the order UpNext prefers to launch from. `primary_source` and the flat `source`, `source_id`, `launcher`, `run_url` and `store_url` describe the preferred copy: installed first, then Steam, GOG, Epic and Amazon.
- `installed` is true when any copy is installed. `playtime_minutes` is the most any copy that tracks it reports.
- Titles are grouped by Steam appid when it's known (natively or by a name match), otherwise by the normalized name.
- Statuses, picks and HowLongToBeat belong to the title, so `exclude_ids` and a pick's `id` are title ids.

## Editions

Each copy carries the store's own `name` and an `edition` (`"Remastered"`, `"Ultimate Edition"`, `"Game of the Year Edition"`, `"Director's Cut"`… or null for the plain release). `GameView.editions` is the list of distinct editions across the copies. Remasters and editions of one game group into one title, even within one store, and the title's `name` is the plain release's when there is one:

```json
{"id": 9, "name": "Alan Wake", "editions": ["Remastered"],
 "copies": [
   {"game_id": 1300, "source": "steam", "name": "Alan Wake", "edition": null, "...": "..."},
   {"game_id": 13, "source": "epic", "name": "Alan Wake Remastered", "edition": "Remastered", "...": "..."}
 ]}
```

## Merging and splitting by hand

Both last through later syncs and regrouping. To find a title to merge with, search with `GET /api/games?q=...&limit=10`.

### POST /api/titles/merge

`{"ids": [9, 412]}`: two or more title ids, the first being the one that stays. Answers with the merged `GameView`.

The title that stays keeps its own name, Steam appid, status, note, ending switch and Play later, through later syncs too. Another title's status, note or switch is taken only where it has none, and an unlike game merged in never renames it. Ratings, stop reasons, favourites, Second Chance and hidden gem rows, verdicts and wishlist rows follow the same rule or move across.

`400` for fewer than two ids or an id named twice. `404`, with nothing merged, for a title the player doesn't see (hidden from them, or by them) or one that isn't there, since a merge changes every player's library.

### POST /api/titles/{id}/split

`{"game_id": 13}`, a `copies[].game_id` of that title. Answers `{"title": GameView, "detached": GameView}`.

The detached copy's new title gets back what its old title had before a merge folded it in: each player's status and note, rating, Play later and hiding, and the ending switch. The title it leaves keeps its own. `400` when the title has a single copy; `404` when the copy isn't part of it.
