# Second chances

[API index](../API.md) · [Statuses](statuses.md) · [Lineups](lineups.md)

The Second chance part of the Lists page's Abandoned tab shows abandoned games whose reasons might no longer hold. For each, "Then" is why you stopped (`stop_reasons`) and "Now" is what player reviews written since say about it, written by the AI in the Second chances job.

- **On the page:** every `abandoned` game, inferred or marked, unless you've answered for it there since you last played it. Playing it again, and putting it down again, brings it back.
- **"Now"** is written from the reviews posted after it was put down (when it was last played, else when it was marked or found abandoned): up to 8, newest first, plus the 3 most helpful from before, each with its year. With no review since, there's nothing to check against and no call is made.
- **The `second_chances` task** (off until someone turns it on; the page's Check now runs it) goes over the games on the page and the ones shelved again for a second try. It reads their Steam and GOG reviews again where the stored ones are over 90 days old (always on `refresh`), then writes "Now" for each game with reviews since that has none or an outdated one, 4 games a call (kind `second_chances`, counted per game, with the summaries' backend and model). A refresh writes every one again. With no AI set up it logs that it was skipped.

## GameView fields

`second_chance` is null, or:

```json
{"now": "2025 and 2026 reviews say most of the bugs are smoothed out…", "verdict": "partly",
 "line": "2025–2026 reviews: crashes mostly fixed, some jank remains.", "written_at": 1790000000, "model": "claude-sonnet-5-5",
 "outdated": false, "answer": null, "answered_at": null}
```

- `verdict` is `better` (what stopped you is fixed or gone), `partly`, `same` (still there) or `unclear`. With no reason about the game (only life reasons, or none) it's only ever `better` or `unclear`.
- `outdated` is true once the reasons or the reviews it was written from have changed.
- `answer` is the page's: `willing`, `still_no` or `not_for_me`.

`second_try` is true for a game shelved again with "Willing to try again".

## In picks

- A second chance whose up-to-date `verdict` is `same` isn't offered.
- "Willing to try again" marks the game `shelved`, so it's offered like any shelved game rather than as a second chance, and one rejection in picks abandons it for good (`abandoned_by_picks`); it doesn't come back to the page. Marking it Shelved again later from the menu gives it the usual two.
- Still no counts as a rejection: the game stays `abandoned` and out of picks.
- The candidate line carries the gist: `second chance: abandoned 2024 after 3.5h; stopped: bugs or performance, "kept crashing"; reviews since: 2025–2026 reviews: crashes mostly fixed, some jank remains.`, or `willing to try again: …`. A lineup picked without AI adds the same to the reason.

## GET /api/second-chances

```json
{"games": [{"game": "GameView", "stopped_at": 1705104000, "reviews_since": 8, "due": false}],
 "ai": {"available": true, "reason": null},
 "task": {"schedule": "off", "enabled": true, "last_run": "Job", "next_run_at": null}}
```

Games where reviews say things changed come first (`better`, then `partly`), then the ones not checked or unclear, then `same`; the most recently put down first within each. `game` is the slim list shape. `due` means the next run writes its "Now". `ai` is the summaries' backend's state, and `task.enabled` the `auto_sync` setting.

## POST /api/second-chances/{id}

Any one of `{"answer": "willing" | "still_no" | "not_for_me"}`, `{"undo": true}` or `{"reasons": {"codes": ["bugs"], "note": "your reply"}}`. Answers with the `GameView`.

- `answer` is for a game on the page, else a `409`. `willing` marks it `shelved`, `not_for_me` marks it `hidden`, and `still_no` leaves the mark alone.
- `undo` takes the answer back, putting back the mark the game had before, as it was (or none).
- `reasons` are checked as for [marking a game](statuses.md#marking-a-game) and stored with source `second_chance`; the page's reply box is their note. Clearing every code and the note records that, so no reasons stand.
- An unknown answer, an answer with `undo`, or none of the three is a `400`; an unknown game a `404`.
