# Patterns, and how picks read the profile

[API index](../API.md) · [Taste profile](taste.md) · [Taste sources](taste-sources.md)

## Patterns

- A game counts as one you **stuck with** when it's a favourite, a staple, beaten, played through (`played` with an ending, past 80% of its main story), still played (`still_playing`), or chosen from picks and played since. It counts as one you **left** when it's `abandoned`, `shelved`, `hidden`, or untouched or sampled and turned down with "It's too ___". A shelved or abandoned game whose reasons are all life reasons doesn't count, and nor does anything else.
- For each review-tag value, each tag on UpNext's lists and each length, the share of its games you stuck with is compared with your share over all of them. Each game weighs `0.5 ^ (age / pattern_half_life_days)`, its age counted from when you set its status, chose it from a pick, or else last played it (not from when UpNext found it shelved or abandoned). A favourite weighs 1 and a game with no date a half. The share has a prior worth 2 games at your usual share.
- A pattern needs `pattern_min_games` games on its side that still count a quarter or more (two half-lives: two years by default), odds of sticking with these games at least twice your usual odds or at most half of them, and at least 80% odds that the difference is real (the beta distribution's tail): `confidence`. 95% and over is `sure`.
- Two patterns the same way where 80% of the smaller one's games are in the other show once, the surer one. At most 15 show.
- A subject you've made a like, dislike or hard no isn't a pattern.
- The `taste` job stores them, and editing a statement or deleting a source counts them again at once. One no longer found is removed, unless you hid it: "Not really" holds when it comes back.

## How picks read the profile

`POST /api/picks`, new spares (`POST /api/picks/{batch_id}/more`) and `POST /api/versus/decide` read the profile:

- **Hard nos** UpNext can check (a review-tag value, a tag, a length) leave a game out of the pool, so it's never offered and isn't counted in `candidates_considered`. A favourite is never left out. Hard nos in your own words go to the AI only.
- **The ranking** that orders the AI's candidates and makes picks without AI: each like a game matches adds 12, as a request's tag match does (at most 2 count); each dislike takes off 8 and each pattern of games you drift away from 5 (at most 2 of each), unless the request asks for that subject; and a game sharing 2 or more things (review-tag values, tags, length) with a favourite gets 2 a shared thing, up to 6.
- **With no time in the request**, the usual session (the `weeknight` habit, or `weekend` from Friday 5 pm to Sunday, in minutes) stands in for one.
- **The prompt** gets About me in place of a most-played list: favourites with why, likes, dislikes and hard nos, the patterns shown on the page, your most-played games while you have fewer than 5 favourites, and the habits you stated. A vague request ("anything", "surprise me", "something good", or none) is marked as one, and a rule tells the AI to lean on About me for it. Help me decide gets the same About me.
- **Without AI**, `ranked_by` can add `your hard nos, left out`, `what you like, on your profile`, `likeness to your favourites`, `what you dislike, kept a little lower` and `the kinds of game you drift away from, kept a little lower`, and its budget line can read `HowLongToBeat's main-story times, for your usual weekend session of 3 hours`. Reasons can add `cozy, which you like` and `like Hades, a favourite`.

## One change of pace

A vague request for 3 or more picks, with likes or patterns of games you stick with on your profile, gets one change of pace: a game outside your favourites, likes and those patterns, never a hard no. The AI is told to make the last pick one and say so in its reason. Picks without AI keep the last slot for the best-ranked such game (unless an earlier pick is one already), with the reason starting "A change of pace, outside your usual lane" and the `ranked_by` line `one change of pace, outside your favourites, likes and the patterns you stick with, as the request was vague`.
