# First-run questions and the play diary

[API index](../API.md) · [Taste profile](taste.md) · [Taste sources](taste-sources.md)

## GET /api/taste/questions

What the first-run questions (`/profile/questions` in the UI) ask. The setup wizard asks them in its "Your taste" step once the library has games, the profile page links to them, and the Pick page points there while nothing is stated.

```json
{"most_played": [{"game": "GameView", "favourite": 18}],
 "few_played": false,
 "loves": [{"about": "facet:mood:cozy", "label": "Cozy", "group": "Mood", "count": 21}],
 "hard_nos": [{"about": "tag:Horror", "label": "Horror", "group": "Genres and themes", "count": 7}],
 "diary": {"on": false, "steam": true}}
```

- `most_played`: your 12 most-played games, recent play weighted up (favourites included), each with its favourite row's id when it is one.
- `few_played`: fewer than 6 games with 2 hours or more, so the UI leans on searching for a game rather than showing covers.
- `loves`: the moods found in the library, then the 18 tags on the most games, with how many games each matches. They're saved as likes.
- `hard_nos`: Horror, Scary, Punishing, Souls-like, Heavy grind, PvP, Massively Multiplayer, Battle Royale, Sports, Racing, Survival, Zombies, Visual Novel and Dating Sim, each with how many games a hard no on it would leave out. Ones that leave nothing out are dropped, unless you already said it, or unless every one of them would leave nothing out.
- `diary.steam`: a Steam Web API key and Steam ID are set, so the diary can read something.

Every answer is saved as it's given, through the routes it belongs to: favourites and why with [`/api/taste/statements`](taste.md#post-apitastestatements), session lengths and who you play with as habits, and the diary with `PUT /api/taste/diary`.

## The play diary

Off by default (the `play_diary` setting). When it's on, the server asks Steam's `IPlayerService/GetRecentlyPlayedGames` about once an hour (it needs schedules on and the Steam key and ID) and writes the minutes each game gained since the last reading:

- The minutes go in the hour halfway between the readings, at most the minutes between them. A reading more than 3 hours after the last one records nothing and becomes the new starting point, since the server was off and the play can't be placed. A game new to the list counts its two-week minutes the same way.
- A Steam game maps to its library title by appid; one not in the library keeps its name and appid.
- The Play button (the game sheet, copies, Versus, lineups, series) writes a `launch` row while it's on. That's the only record of Epic and Amazon games through Heroic, which report no playtime.
- The profile reads it for the habits it can see over the last 90 days: how long weeknight and weekend sessions run (consecutive hours with play are one session; the median, to the quarter hour; at least 3 sessions), when you play (a time of day with a fifth of the minutes or more; at least 5 hours of play), and when multiplayer games get played (an hour or more of games tagged Multiplayer, Co-op, PvP, Massively Multiplayer, MMORPG, MOBA or Battle Royale). A habit you state wins over the diary's; the diary fills the ones you haven't.

The switch is the household's, one for every player, so a player who turns it on or off does so for everyone. What it records is each player's own: Steam rows count for the account's owner and the players who play on it, `launch` rows for whoever pressed Play.

### What the diary shows

`GET /api/taste` carries `diary`:

```json
"diary": {"days": 16, "minutes": 1640, "weeknight": {"minutes": 60, "count": 8}, "weekend": {"minutes": 165, "count": 8},
          "times": ["evenings"], "multiplayer": {"minutes": 740, "weekend_share": 0.65, "time": "evenings"}}
```

Each part is null, or empty, until it has enough. Picks and Help me decide read it in About me ("(from my play diary)"), and as the usual session when no habit is stated.

### PUT /api/taste/diary

`{"on": true}` turns it on and takes the first reading at once when the Steam key and ID are set; a failed reading shows in `diary.error` and doesn't stop the switch. `{"on": false}` turns it off and forgets the last reading, so turning it on again starts afresh; the rows stay until you delete them. Answers `{"diary": …}`, the `diary` of [`GET /api/taste/sources`](taste-sources.md).

### POST /api/taste/diary/launch

`{"title_id": 41}` answers `{"recorded": true}` when the diary is on and `false` when it's off. `400` for a game not in the library.
