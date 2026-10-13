# Wishlist in picks

[API index](../API.md) · [Wishlist](wishlist.md) · [Lineups](lineups.md)

With the `wishlist` setting on, a lineup the AI picks from the whole library (not New to you or Hidden gems, and not one picked without AI) can show two things beyond your library.

## A wishlisted pick

Up to 8 of your wishlisted games (here or on Steam) that you don't own and that aren't a hard no are listed after the request, best fits for its words first, in the candidates' format with `w` ids (`w12 Hollow Knight: Silksong | Action, Metroidvania | ?h | 95%/2.0k | 2025 | says: …`). At most one may stand in for one of the picks, and that pick becomes its `alternate`, so switching needs no second call. In the lineup it's a pick like the others:

```json
{"id": -12, "appid": 1030300, "name": "Hollow Knight: Silksong", "reason": "…", "game": null, "slot": 1, "feedback": null,
 "owned": false,
 "wishlist": WishGame,
 "alternate": {"id": 1145360, "name": "Hades", "reason": "…", "game": "GameView"}}
```

- `id` is minus its wishlist entry's id, since no title has a negative id.
- A `WishGame` is what the card shows: `{"id": 12, "appid": 1030300, "name": "…", "source": "manual", "state": "wishlisted", "on_steam": false, "short_description": "…", "genres": [], "tags": [], "release_date": "…", "header_image": "…", "portrait": "…", "review_pct": 95, "review_total": 2000, "review_desc": "…", "hltb_main": 30.0}`, with up to 8 tags. Once its wishlist entry is deleted, the pick keeps a `WishGame` made from its own name and appid and what the store data still holds, with `source: "manual"`, `state: "wishlisted"` and `on_steam: false`.
- A wishlisted pick that doesn't hold up (not listed, an empty reason, standing in for something that isn't a pick) is dropped and the lineup stands as picked, with no second call.
- Its card's one action is the switch: `feedback` and `too` answer `400` for it. New spares never list the wishlist, and treat the alternate as still in the lineup.

### POST /api/picks/{batch_id}/switch

"Switch with owned game": `{"title_id": -12}`, the wishlisted pick's `id`. Its `alternate` takes the slot, with its reason, at once and with no AI call. When the alternate can't go on screen any more (marked since, or of a series a spare brought on), the best usable spare does, as for Not tonight. The wishlisted pick moves to `removed` with `owned: false` and no `feedback`. Answers with the lineup. `400` for a game you own or one with no alternate; `404` for an unknown lineup or a game not on screen.

## The suggestion

The pick call may name one game you don't own, from the AI's own knowledge, when it fits the request and your taste unusually well. UpNext checks it before it's shown, each request tried once and 12 seconds at most in all:

- Steam's store search finds it by name (the strict match used elsewhere), and Steam says it's a game;
- 80% or more of at least 50 Steam reviews are positive;
- you don't own it on any synced store (by name before the search, then by the store's appid and name, editions folded in);
- you haven't answered it: on the wishlist already, Not interested, I own it elsewhere or in your library all drop it, and one already waiting as a suggestion is shown;
- it isn't a hard no, by Steam's tags and its store genres.

Once it passes, it waits on the wishlist as a suggestion (`source: "pick"`, `batch_id` the lineup), with its store data and art fetched, and the lineup carries it:

```json
"suggestion": {"game": WishGame, "reason": "Not in your library, but …"}
```

`game.state` is where it stands now, so a lineup read later shows the answer given since (`PUT /api/wishlist/{id}` answers it, as on the Lists page). A suggestion that fails a check, or that Steam doesn't answer for in time, isn't shown (the server log says why) and nothing is stored. `suggestion` is null otherwise, and always for lineups picked without AI or in a mode.
