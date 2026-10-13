# Wishlist

[API index](../API.md) · [Wishlist in picks](wishlist-picks.md) · [Series](collections.md)

Games you don't own: suggestions from the series you've played, games you add by name, and your Steam wishlist, read only. It isn't a store, so there are no links, prices or sales. Everything here is behind the `wishlist` setting (on by default; Settings › Picks, "Wishlist and suggestions"). With it off there are no suggestions, no Steam wishlist read and no store data fetched, the Lists tab is hidden, and `GET /api/wishlist` and `from_wishlist` are empty. What's stored stays until it's deleted from [What UpNext knows about you](taste-sources.md).

## Where rows come from

- **Your series** (`source: "collection"`): a series' entries you don't own that have a Steam appid on Wikidata, where the series means something to you: at least 10 hours played across its games you own, or one of them beaten or a favourite. The reason names that history ("You played Half-Life and Half-Life 2 for 25 hours. This one came after them, in 2007."). Most-played series first; at most 3 suggestions from one series and 30 in all wait at once.
- **Your Steam wishlist** (`source: "steam"`, `on_steam: true`), read with `IWishlistService/GetWishlist` for your SteamID (with the Web API key when there is one), names from the store's `GetItems`. A game on it is never a suggestion: a suggestion that turns up there is wishlisted. A game taken off it on Steam loses `on_steam`, and one that only came from Steam is dismissed. A private or empty answer leaves the copy as it was. UpNext never writes to Steam.
- **By name** (`source: "manual"`), from `POST /api/wishlist`, found with Steam's store search.
- **A lineup** (`source: "pick"`, `batch_id` the lineup): the AI's suggestion of a game you don't own ([Wishlist in picks](wishlist-picks.md)).

A suggestion is shown only when it's a game on Steam's store, 80% or more of at least 50 Steam reviews are positive, you don't own it on any store UpNext syncs (by Steam appid, then by name with editions folded in: "Dishonored" is owned by "Dishonored - Definitive Edition", "The Witcher 3" isn't), you haven't answered it, and it isn't a hard no (by its five strongest Steam tags or its store genres).

## The `wishlist` task

"Update the wishlist" runs after Fetch details, whose series it reads. It takes games now in the library off the wishlist, reads the Steam wishlist, checks your series (reviews first, so a poorly rated game costs one request), then fetches what each row lacks: store details, the review summary, Steam tags, art, and a HowLongToBeat time by name (kept on the row; three failures in a row stop it until the next run). No AI is used. Its log says what it found ("Your series: 12 games you don't own could be suggested; 4 new suggestions, 8 passed over…"). A sync, and `GET /api/wishlist`, also take games now in the library off the wishlist.

## GET /api/wishlist

```json
{"enabled": true, "suggested": [WishlistEntry], "wishlist": [WishlistEntry],
 "dismissed": 3, "owned_elsewhere": 1, "in_library": 2, "steam_account": true,
 "task": {"schedule": "after:details", "enabled": true, "last_run": "Job", "next_run_at": null},
 "rules": {"series_hours": 10, "min_positive": 80, "min_reviews": 50, "per_series": 3}}
```

`suggested` wait for an answer, best rated first; `wishlist` is yours and Steam's, newest first. A `WishlistEntry`:

```json
{"id": 4, "appid": 420, "name": "Half-Life 2: Episode Two", "source": "collection", "reason": "You played …",
 "state": "suggested", "on_steam": false, "series_key": "Q…", "series": "Half-Life", "batch_id": null, "title_id": null,
 "hltb_main": 5.5, "added_at": 1790000000, "updated_at": 1790000000, "wishlisted_at": null,
 "type": "game", "short_description": "…", "genres": ["Action"], "developers": ["Valve"], "release_date": "Oct 10, 2007",
 "header_image": "https://…", "portrait": "https://…", "review_pct": 97, "review_total": 31000,
 "review_desc": "Overwhelmingly Positive", "tags": ["FPS", "Action", "Sci-fi"]}
```

States: `suggested`, `wishlisted`, `dismissed` (Not interested: never suggested again), `owned_elsewhere` (I own it elsewhere: counts as owned from then on) and `in_library` (a sync found it, with its `title_id`). `wishlisted_at` is when it went on the wishlist, here or on Steam.

## POST /api/wishlist

`{"id": 4}` puts a row on the wishlist (a suggestion, or one said no to before). `{"name": "Hades II"}` finds a game on Steam's store and adds it, or wishlists the row it has already, and fetches its store data and art at once. Someone is waiting, so each request is tried once, for 8 seconds at most; store data that doesn't come back is left for the next run. Answers with the `GET` body.

`400` for neither, a name Steam's store doesn't know, or a game you own ("Hades II is in your library already, as Hades II"); `404` for an unknown id; `409` for a game in your library now, or with the setting off; `502` when Steam's store doesn't answer the search.

## PUT /api/wishlist/{id}

`{"state": "wishlisted" | "dismissed" | "owned_elsewhere" | "suggested"}`. Answers with the `GET` body. The UI's Remove is `dismissed`. `400` for another state (`in_library` comes only from a sync); `404` and `409` as above.

## DELETE /api/wishlist/{id}

Forgets the row, so a series may suggest it again, and the next read of your Steam wishlist brings it back if it's there. Answers with the `GET` body; `404` for an unknown id. It works with the setting off.

## Elsewhere

`GET /api/games/{id}` carries `from_wishlist` for a game that came into the library off the wishlist: `{"reason": "…", "source": "collection", "wishlisted_at": 1790000000}`. The game sheet says "From your wishlist (Aug 23): <reason>". Merging titles moves a row's `title_id` to the title that stays.
