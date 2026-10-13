# First-run setup

[API index](../API.md) · [What the wizard reads](setup-wizard.md) · [Background work and uploads](setup-background.md)

The UI's `/setup` page is the first-run wizard: welcome, where UpNext runs, Steam, GOG, Epic and Amazon, Epic, EA and Ubisoft, extras, AI, what the AI does, first sync, details, your taste, who plays here, and done. Every page goes there while the library is empty and setup is neither done nor skipped. Settings › Setup has "Restart the setup wizard", which on this computer can clear chosen parts first. The steps reuse the Settings page's rows and its `PUT /api/settings`, `POST /api/jobs` and `PUT /api/jobs/scheduled/{task}`; these routes add what the wizard reads, where it got to, and the jobs it runs in the background.

Each player also has a short setup of their own at `/setup/player` (their name and colour, the games and AI offered to them, the taste questions, the tour) and their own tour. `GET /api/setup/state`, `GET /api/setup`, `PUT /api/setup` and `POST /api/setup/reset` answer for the request's player and take the `UpNext-Player` header.

## GET /api/setup/state

```json
{"needed": false, "step": "done", "setup": "done", "tour": "open", "tour_offer": true,
 "player_needed": false, "player_step": null, "player_setup": "done"}
```

- `needed`: the wizard shows (the library has no titles and `setup` is `open`). The UI checks it before any other page.
- `step`: the step the wizard last reached, or null: `welcome`, `where`, `steam`, `stores`, `more_stores`, `extras`, `ai`, `ai_jobs`, `sync`, `details`, `taste`, `players` or `done`. "Who plays here?" (`players`) first asks the name of whoever runs it, offering [`you.suggested`](setup-wizard.md), and renames them with `PUT /api/players/{id}`, then adds anyone else with `POST /api/players`.
- `setup` and `tour`: `open`, `done` or `skipped`. `tour_offer` is true once setup no longer shows, the library has games and the tour is still `open`, and the UI then offers the tour once.
- `player_needed`: the UI sends the request's player to their own setup before any other page. True with two or more players, once the wizard no longer shows, while their `player_setup` is `open`. The first player's own setup is the wizard, so it starts `done`; anyone else's starts `open`, and whoever ends the wizard has theirs ended the same way. `player_step` is the step they last reached: `you`, `games`, `ai`, `taste` or `done`.
- While a default player with a PIN hasn't been chosen, the state is the household's only: `player_needed` and `tour_offer` are false.

## PUT /api/setup

Any of `{"step": "sync", "setup": "skipped", "tour": "done", "player_step": "games", "player_setup": "done"}`. Answers with the state. `400` for an unknown step. `tour`, `player_step` and `player_setup` are the player's own; `step` and `setup` are the household's, which a token bound to one player can't change (`403`).

The UI sends `{"step": "done", "setup": "done"}` at the last step and `{"setup": "skipped"}` from "Skip setup". "Restart the setup wizard" starts at the welcome step and leaves `setup` as it was. The tour sends `{"tour": "done"}` at its last stop, and `{"tour": "skipped"}` from the offer's "No thanks", the wizard's "I'll explore", or leaving the tour early.

## POST /api/setup/reset

Any of `{"accounts": true, "ai": true, "taste": true, "history": true, "library": true}`. Clears what's chosen before "Restart the setup wizard" opens the wizard; the wizard itself never deletes anything. Answers `{"backup": "upnext-manual-20261009-181500.db", "from_env": ["steam_api_key"]}`: a backup is taken first, kept until deleted, with a note saying what was cleared, and `from_env` lists cleared settings that still have a value from the environment or `.env`, which the app can't clear.

- `accounts`: `steam_api_key`, `steam_id` (with the Steam name and picture), `igdb_client_id`, `igdb_client_secret`, `rawg_api_key` and `galaxy_db`, and the player's GOG and Epic sign-ins.
- `ai`: `llm_backend`, `digest_backend`, and every model, key, address, effort and thinking setting of each backend.
- `taste`: every profile statement (what you said, the patterns found, the ones you hid), routines and the play diary. The patterns are worked out again at once from what's left.
- `history`: the statuses and star ratings you set, why you stopped, Play later, Second Chance answers, "This game can be played endlessly", your verdicts, and every lineup with what was done with its picks and how they were rated.
- `library`: every game and title, how they're grouped, everything fetched or written about them, the wishlist, other players' verdicts and the last sync's time; `taste` and `history` go with it. The settings, the API tokens, the jobs and the record of AI calls stay.

With two or more players, `accounts` clears only the player's own SteamID, Galaxy file and sign-ins (the API keys are everyone's and stay, and the Steam name and picture go only when the first player clears theirs), and `taste` and `history` clear only the player's own: their statements, routines, launches and the readings of the Steam accounts they own, their marks, ratings, reasons, Play later, hidden games, verdicts, lineups and Second Chance answers. "This game can be played endlessly", a fact about the game, stays. `library` is everyone's.

Schedules, looks and the other settings are never cleared. Only on this computer (`403` from another device, whatever its token, so never in a container), never while a job runs (`409`), and `400` when nothing is chosen.
