# Ratings

[API index](../API.md) · [Picks](picks.md) · [Taste profile](taste.md)

Two ratings, each kept apart from what you did with a game. A pick's rating says how well the suggestion fit what its lineup was asked for, not what you think of the game; a game's rating is your own, 1 to 5 stars.

## PUT /api/picks/{batch_id}/rating

`{"title_id": 1234, "verdict": "not_a_fit", "reason": "wrong_length"}`. `verdict` is `good` ("Good pick") or `not_a_fit` ("Not a fit"), or `null` to take the rating back. `reason` goes with `not_a_fit` only and is optional: `wrong_length`, `wrong_mood`, `wrong_genre` or `played_out`. Answers with the lineup.

A rating isn't a card action: the pick stays in its slot, and Play and the other actions still work on it. Any pick of the lineup can be rated, spares and picks taken off included. Each pick has `rating`, `{"verdict", "reason", "at"}`, or null.

`400` for an unknown verdict or reason, or a reason with `good`; `404` for an unknown lineup or a game that isn't in it.

## PUT /api/games/{id}/rating

`{"rating": 4}`, 1 to 5, or `null` to clear it. Answers with the `GameView`, whose `my_rating` is the rating and `rated_at` when it was given. The game page's words: 1 "Disliked it", 2 "Didn't click", 3 "It was fine", 4 "Liked it", 5 "Loved it". `400` outside 1 to 5; `404` for no such title.

`GameView.pick_rating` is the latest rating it had as a pick, from any lineup: `{"verdict", "reason", "at", "prompt"}`, where `prompt` is what that lineup was asked for (empty for none), or null.

## How picks read them

- **Game ratings** are part of the taste profile and never fade. About me lists the games you rated 4 or 5 ("Loved or liked", up to 12) and 1 or 2 ("Disliked or didn't click", up to 8), newest first, each with its rating; a 3 says neither. A rated candidate's line says "rated 2/5 by me".
- **In the ranking**, a rated candidate moves 5 points for each star above or below 3. A game you rated 4 or 5 counts like a favourite for likeness (a candidate sharing 2 or more things with it gets 2 points for each, up to 3), unless a favourite already counted, and one you rated 1 or 2 counts the same way against candidates like it. Reasons say "you rated it 4/5" or "like Hades, which you rated 5/5", and `ranked_by` names "your own ratings of games", "likeness to games you rated 4 or 5" and "likeness to games you rated 1 or 2, kept a little lower".
- **Pick ratings** last 90 days, since they were about a request. The request part of the prompt gets "How recent picks landed", up to 8 rated picks, newest first (`- Hades, for "something short": not a fit (wrong length)`), with a line asking the AI to lean toward what landed for requests like this one. It goes with the request, so the cached candidate list stays the same. In the ranking, a game rated not a fit in that time is 8 points lower; a good pick changes nothing there, as the last few lineups' games are held back anyway.

## Elsewhere

- Merging titles keeps the surviving title's rating, or takes the other's when it has none; pick ratings follow their games.
- `GET /api/games` takes `rating` and `sort=rating` ([The library](library.md)), and `GET /api/stats` gives `rated`. `upnext list --sort rating` sorts the same way.
- [Taste sources](taste-sources.md) list, delete and export them: pick ratings with the reactions, game ratings as `ratings`.
