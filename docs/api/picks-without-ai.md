# Picks without AI

[API index](../API.md) · [Picks](picks.md) · [Lineups](lineups.md)

With no AI set up (the chosen backend's own check fails: no `claude` or `codex`, no key, no server address or model), or with `"without_ai": true`, `POST /api/picks` makes the lineup from UpNext's own ranking, the one that orders the AI's candidate list, with no AI call. `upnext pick --without-ai` does the same in a terminal, and so does `upnext pick` with no AI set up.

The lineup is the same shape, with:

- `ranked_by`: what the ranking went by, in plain words, and only what counted for some game it could choose from, such as `["Steam review scores, else GOG's rating, and how many reviews", "HowLongToBeat's main-story times, for 2 hours", "your words against tags, genres and review tags, then names, blurbs and review summaries", "a little chance, so each lineup differs"]`. A reason in `avoid` it can't read ends the list: `not "I just finished a long RPG": only the AI reads reasons in your own words`.
- `model` null, `attempts` 0, `usage`, `cost_usd` and `api_usd` null, and `candidates_considered` every game it could choose from.
- Each pick's `reason` built from the data: `97% of 80k Steam reviews positive · 3.5h main story, about 2 sittings of 2 hours · relaxed pace, as asked`, starting with `You stopped: …` for a shelved game or a second chance with reasons.

The lineup is the top `count` of the ranking, then `2 × count` spares, with one game per series and one second chance (unless asked for more), as with the AI. Each lineup varies the ranking's jitter, so lineups differ, and games shown in the last 3 lineups rank lower.

`avoid` is read where it can be:

- `too <word>` (the menu's "Too long" and "Too slow", and the words said with "It's too ___") holds back games the word rules out against the excluded games, as [`/too`](lineups.md#post-apipicksbatch_idtoo) reads words;
- "Want something newer" (or older) holds back games no newer (or older) than them;
- "Not in the mood for these genres" holds back games sharing their genres;
- "Want something …" adds its words to the request.

On a lineup picked without AI, or with no AI set up:

- `POST /api/picks/{batch_id}/too` makes no call: the first usable spare the word doesn't rule out takes the slot, no other pick is replaced, and `ai_error` is null, since nothing failed.
- `POST /api/picks/{batch_id}/more` takes the next games in the ranking, past every game the lineup has held, steering away from games like the ones said to be "too ___".

Help me decide stays an AI feature: with no AI set up it answers `503` with the reason.

The taste profile, ratings, routines and other players' verdicts add their own lines to `ranked_by` and the reasons; their pages say which.
