# Routines

[API index](../API.md) · [Picks](picks.md) · [Taste profile](taste.md)

A routine is a named session that puts together what the habits keep apart: when, who with, where, on what and for how long ("Friday with friends": Friday evenings and late nights, with friends, at a desk, on keyboard and mouse, 2 hours). They're edited on the Profile, after Habits, and the Pick page offers them as chips, with the one that fits the time chosen to start with. A routine is personal, like the habits: it's only what you say.

```json
{"id": 3, "name": "Friday with friends", "days": ["fri"], "times": ["evenings", "late_nights"],
 "who": "friends", "place": "desk", "input": "keyboard_mouse", "minutes": 120, "note": null}
```

- `name`: up to 40 characters, and no two the same, ignoring case.
- `days`: any of `mon` to `sun`, Monday first; empty for any day.
- `times`: any of `mornings` (5 to 11 am), `afternoons` (12 to 4 pm), `evenings` (5 to 9 pm) and `late_nights`, earliest first; empty for any time.
- `who`: `solo`, `partner`, `friends` or `kids`. `place`: `desk`, `couch` or `deck` (a Steam Deck). `input`: `keyboard_mouse` or `controller`. Each may be null.
- `minutes`: 15 to 720, or null. `note`: up to 300 characters, read by the AI only.

At most 20 routines.

## GET /api/routines

`{"routines": [Routine], "now": 3}`, in the order they were made. `now` is the routine that fits the day and time of day now (local time), or null. Only a routine with days or times can fit (one with neither is only chosen by hand); when several do, the narrowest wins (fewest days times times of day), then the first made. The small hours before 5 am count as the night before, so a Friday late-night routine fits at 1 am on Saturday.

## POST /api/routines

A routine without `id`; only `name` is required. Answers `201` with the routine.

`400` for a name that's empty, too long or taken, a length out of range, a note too long, a 21st routine, or a body it can't read (no `name`, a day, time of day, `who`, `place` or `input` it doesn't know, a length that isn't a number, or JSON that doesn't parse), with the reason in `error`. A body not sent as `Content-Type: application/json` is a `415`.

## PUT /api/routines/{id}

A routine, as for `POST`, replacing the one there. Answers with the routine. `400` and `415` as for `POST`; `404` for no such routine.

## DELETE /api/routines/{id}

`204`; `404` for no such routine.

## How picks read a routine

`POST /api/picks` with `routine_id` picks for that routine. The lineup keeps the routine as it was then (`routine` on the lineup), and its new spares keep to it even after the routine changes or is deleted.

- **The prompt**: the request part gets "This session": `My routine "Friday with friends" (Friday evenings and late nights): with friends, at a desk, on keyboard and mouse, about 2 hours.`, then a line for each part it says ("I'm playing with friends: games we can play together, co-op or multiplayer.", "On a controller: games that play well on one, not games made for a mouse.", "About 2 hours: with no time in my request, that's my time budget.", the note) and "This session outweighs the habits in About me; my request outweighs both." The system prompt asks for every pick and spare to fit it unless the request says otherwise.
- **The ranking** reads each game's tags, Steam store categories and IGDB modes, and leaves alone a game that says nothing:
  - With others (`partner`, `friends`, `kids`), a co-op or multiplayer game is 15 points higher, and one that says single-player and nothing else is 25 lower with friends, who can't share it, or 10 lower with a partner or kids, who might watch a story unfold. On your own (`solo`), a game played online against or among others, with nothing saying it can be played alone, is 15 lower. With kids, a game tagged gore, violent, sexual content, nudity, mature or horror is 20 lower.
  - On a controller (`input: controller`, or `place: deck` with no input said), full controller support is 10 points higher, partial (or the "Controller" tag) 4, and a game whose Steam store details list no controller support 10 lower. Keyboard and mouse changes nothing.
  - Its length is the time budget when the request gives none, ahead of the usual session from your habits.
  - A reason says "co-op or multiplayer, for playing with friends" or "full controller support", and `ranked_by` names the routine with what it counted.
- **The command line**: `upnext pick --routine "Friday with friends"` and `upnext prompt --routine …`, by name in any case. `upnext taste` lists the routines and which fits now.
