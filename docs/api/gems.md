# Hidden gems

[API index](../API.md) · [Discovery](discovery.md) · [Jobs](jobs.md)

The `gems` pick mode, and the shelf on the Lists page, draw from unusual games the AI has rated.

- **The pool:** unplayed (untouched) games picks offer, not endless and not a hard no, not known to take more than a session or two, and rated by `gem_min_positive` percent (93) of at least `gem_min_reviews` (500) Steam reviews or, with no Steam reviews, 4.5 of 5 from 200 GOG ratings. Length is a HowLongToBeat main story of `gem_max_hours` (5) or less. With no HowLongToBeat time (it's off by default), players' review tags decide (short or long sessions), then Steam's tags: "Short" is short, and an open world, an RPG, or a survival, sandbox, management or builder game among its five strongest tags is long. A game nothing speaks for stays in, after the ones known to be short.
- **The `hidden_gems` task** (off until turned on, like every AI job; the Lists page's Find hidden gems runs it) has the AI rate each game in the pool not rated yet, or rated by an older prompt (all of them on `refresh`), 40 games a call (kind `hidden_gems`, counted per game, with the summaries' backend and model). Each gets `studio` (how little-known, from the developer, the publisher and whether it's self-published), `mechanics` (how rare, from its three rarest tags and how many games in the library carry each) and `play` (how odd, from People say and Vibes), each 0 to 3; `party` for a game made for a group; and a `line` on what makes it unusual, in the Vibes voice. With no AI set up it logs that it was skipped.
- **In picks:** party games are left out, a rated gem's candidate line ends "| unusual: <line> (studio 3/3, mechanics 2/3, play 2/3)", and the ranking without AI adds 3 points per rating point.

## GET /api/gems

```json
{"gems": [{"game": "GameView",
           "rating": {"studio": 3, "mechanics": 2, "play": 3, "party": false, "line": "You unpack a life, box by box.",
                      "prompt_version": 1, "model": "claude-sonnet-5-5", "written_at": 1790000000},
           "score": 8, "rarest": [["Wholesome", 2], ["Cozy", 5], ["Pixel Graphics", 8]]}],
 "pool": 80, "rated": 80, "unrated": 0, "party": 6,
 "ai": {"available": true, "reason": null},
 "task": {"schedule": "off", "enabled": true, "last_run": "Job", "next_run_at": null},
 "rules": {"min_positive": 93, "min_reviews": 500, "max_hours": 5.0, "count": 12}}
```

The shelf is the `gem_count` (12) best-rated games in the pool that aren't party games, by the three ratings together, then review score, then review count. A gem you play leaves the pool and the next moves up. `unrated` is what the next run rates; `party` the rated party games left off. `ai` says whether the summaries' backend can run, and why not. `task.enabled` is the `auto_sync` setting.

`?all=true` adds `ranked`: every game in the pool, the shelf's order first, then party games, then the ones not rated yet (`rating` and `score` null), for tuning what "unusual" means.

## Settings

`gem_min_positive` (93, 50 to 100), `gem_min_reviews` (500, 0 to 1,000,000), `gem_max_hours` (5, 1 to 200) and `gem_count` (12, 1 to 100), in [Settings](settings.md).
