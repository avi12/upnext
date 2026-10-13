# Discovery: new to you, and roll the dice

[API index](../API.md) · [Picks](picks.md) · [Hidden gems](gems.md)

Three pick modes narrow the candidates: New to you and [Hidden gems](gems.md) draw from the library, and Roll the dice draws by chance. The first two are asked for in the request's own words ("New to me", "new to me, 2 hours", "A hidden gem for tonight"; whole words, so "brand new to medieval games" isn't) or with `mode` on `POST /api/picks`. Picks keep their usual rules (one per series, one second chance), the rest of the request still counts, and new spares (`…/more`) stay in the lineup's mode. Too few games for the count is a `400`: "no candidates: only 1 of your games is new to you (…), cannot pick 3".

## New to you (`new_to_you`)

- **New to you** is a genre with under `unfamiliar_share` percent (2) of your played hours and fewer than 2 played games, or a mechanic on none of the games you've played or sampled. Genres and mechanics come from fixed lists (`discovery::new_to_you::GENRES` and `MECHANICS`), read from a played game's ten strongest store tags and its store genres. A game is a way into one when it's among its five strongest tags or its store genres.
- **Candidates** are games picks offer as new (untouched or sampled), not a hard no, a way into something new to you that you don't dislike or rule out (a dislike or hard no on Horror covers Survival Horror), and gentle. Gentle means review tags don't say hard, punishing, steep or deep (when they don't say how hard, Difficult, Souls-like or Precision Platformer without Casual or Relaxing rules it out), and a main story of `new_to_you_max_hours` (8) or less, or `sessions:short` when HowLongToBeat has no time. An endless game needs a gentle learning curve instead, or, when reviews don't say, simple systems or a Casual or Relaxing tag.
- **A lineup spreads**: each pick stands for a different genre or mechanic (spares are exempt, as one only takes a pick's place). The AI is asked again when two picks can't. Picks without AI keep such a game as a spare, and when too few games can make the count the pick is a `400` ("only 2 of the eligible games can share a lineup (one game per series, each a way into something different), cannot pick 3") rather than a lineup that repeats one.
- A candidate line ends "| new to you: deckbuilders (a mechanic you haven't tried)", the AI is asked for a reason that says what the genre is like and why this one is a gentle start, and a reason without AI starts "New to you: deckbuilders".

### GET /api/new-to-you/{id}

```json
{"ways": [{"about": "tag:Deckbuilding", "tag": "Deckbuilding", "phrase": "deckbuilders", "kind": "mechanic"}]}
```

The genres and mechanics a game is a way into for you now, mechanics first, leaving its own play and the gentleness rules aside. On "Not for me" for a New to you pick, the UI offers to make the first of these a hard no: `about` goes straight into [`POST /api/taste/statements`](taste.md#post-apitastestatements). `404` for a game not in the library.

## Roll the dice (`random`)

Only `"mode": "random"` asks for it; no words do, and the request's words don't steer it. The Pick page's dice button sends it with `count` 1 and an empty request.

- **Off your radar** is a game picks offer that you haven't started, or that you last played (or, with no playtime, marked) at least a year ago: only tried, shelved, or abandoned and still owed its second chance. Endless games and staples never are, and nothing picks leave out comes up (Playing, Beat it, Not for me, hard nos).
- **The draw** gives every eligible game the same chance. It leaves out `exclude_ids`, the picks of the last three lineups (unless that leaves too few) and, with Linux on, games ProtonDB rates borked, and keeps one game per series and one second chance. Each reason says why it's off your radar ("Drawn at random from the games you haven't started.", "Drawn at random: you put it down 2 years ago."). Spares, `count` × 2, are drawn the same way.
- **The lineup** is saved like any other, with no AI call even when one is set up: `model` null, `attempts` 0, `candidates_considered` the games it drew from, and `ranked_by` one line, "chance alone: drawn at random from 42 games off your radar". So History calls it picked without AI, "It's too ___" takes the next spare the word doesn't rule out, and new spares are drawn at random again. Too few games is a `400`: "no candidates: none of your games is off your radar yet (…)".

## Settings

`new_to_you_max_hours` (8, 1 to 200) and `unfamiliar_share` (2, 0 to 100, percent), in [Settings](settings.md).
