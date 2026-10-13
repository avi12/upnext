# Picks

[API index](../API.md) · [Lineups](lineups.md) · [Picks without AI](picks-without-ai.md)

## POST /api/picks

```json
{"prompt": "I have an hour and want something relaxing", "count": 3, "exclude_ids": [1234],
 "sources": ["steam", "gog"], "avoid": ["Too long"], "without_ai": false,
 "mode": null, "routine_id": null, "input": null}
```

Every field is optional:

- `prompt`: the request; empty for a general recommendation. At most 2,000 characters.
- `count`: how many picks to show, 3 unless asked, kept within 1 to 5.
- `exclude_ids`: title ids to leave out, for Curate again: the picks on screen and the ones taken off. At most 500.
- `sources`: the stores to pick from; all of them when left out.
- `avoid`: why the excluded games were turned down, such as "Too long". The prompt names those games with the reasons and tells the AI to steer away from them. At most 50 reasons of at most 300 characters each.
- `without_ai`: pick from UpNext's own ranking with no AI call, even with an AI set up ([Picks without AI](picks-without-ai.md)). With no AI set up, picks always come from the ranking.
- `mode`: `new_to_you`, `gems` or `random` ([Discovery](discovery.md)). A request that says "new to me" or "a hidden gem" picks in that mode without it; `random` is only asked for here.
- `routine_id`: the routine this session is ([Routines](routines.md)); `404` for an unknown one.
- `input`: `controller` when the request comes from big screen, so it counts as played on a controller, as a routine on one does: Steam's full or partial controller support ranks a game higher and a game whose store categories list none lower, `ranked_by` says so, and the request gets the line "On a controller: games that play well on one, not games made for a mouse." A chosen routine that names its own input outweighs it, and `keyboard_mouse` changes nothing. Any other value is a `422`.

A request over a limit is a `400` naming it. The whole request goes to the AI and into History, and the ranking compares every word and id with every game, so the limits keep a stray paste from making a costly call or stalling the server.

The AI call takes roughly 20 to 90 seconds. The answer is a lineup:

```json
{
  "batch_id": "b3f2...", "prompt": "...", "created_at": 1758990000,
  "model": "sonnet", "duration_ms": 41000, "cost_usd": null, "api_usd": 0.12,
  "candidates_considered": 120, "attempts": 1,
  "usage": {"input_tokens": 29212, "output_tokens": 302, "cache_read_tokens": 0, "cache_write_tokens": 29210, "cache_write_1h_tokens": 29210},
  "ranked_by": null, "mode": null, "routine": null,
  "shown": 3,
  "picks": [{"id": 1234, "appid": 620, "name": "Portal 2", "reason": "Short, funny, ...", "game": "GameView",
             "slot": 0, "feedback": null, "rating": null, "owned": true, "backlog": null}],
  "spares": [{"id": 77, "...": "...", "slot": null, "feedback": null, "owned": true}],
  "removed": [],
  "suggestion": null
}
```

- `picks`, `spares`, `removed` and `shown` are the lineup, which [Lineups](lineups.md) explains. A pick's `rating` is in [Ratings](ratings.md) and `backlog` in [Backlog verdicts from others](community-verdicts.md#on-games-and-picks).
- One pick may be a wishlisted game you don't own (`owned: false`), and `suggestion` may name one ([Wishlist in picks](wishlist-picks.md)).
- `usage` is the tokens every call made with the lineup used, a retry included; null when the backend didn't say. `cost_usd` is what they were charged and `api_usd` what they'd cost at list prices ([AI usage](ai-usage.md)).
- `ranked_by` is set on a lineup picked without AI, and `model` is then null. `mode` is its discovery mode, and `routine` the routine it was picked for, as it was then.

Errors:

- `400`: no candidates (sync and fetch details first), or too few for the count in a mode.
- `429`: the AI's provider takes no more calls for now (a spent usage limit, no credit left, rate limiting that outlasted the retries), or another pick of yours is running.
- `502`: the AI failed, or returned picks that don't hold up twice.
- `503`: the AI stopped being available during the call (signed out, a model that doesn't exist). With no AI set up at all (`claude_available: false` in [`GET /api/config`](basics.md#get-apiconfig)) there's no `503`: the lineup comes from the ranking.

### The prompt and its cache

The prompt is a cacheable prefix (the taste profile and the candidate list, which depends only on the request's words, the stores and the day) and then the request. A Curate again keeps the turned-down games in the list and names them as off limits, so the provider can reuse the prefix.

## GET /api/picks/history

Past lineups in the shape above, newest first, or oldest first with `order=oldest`. Every parameter is optional:

- `limit` (1 to 200, 20 unless asked) and `offset` (0). A page shorter than `limit` is the last.
- `since`: only lineups made at or after this time, in Unix seconds.
- `q`: only lineups whose request, or the name of a game they showed (in its slot, or taken off with a reason; not an unused spare), has every word of it, up to eight words. Case is ignored for ASCII letters, and `%` and `_` match only themselves.
- `outcome`: `played` (one of its games got Play or "I've started it"), `saved` (one of the games it showed is on Play later now) or `turned_down` (one got Not tonight, "It's too ___" or Never again; a pick the same "It's too" call took off doesn't count).

A value it can't take is a `400`. `usage` is null after a year, when the call records are gone.
