# A verdict

[Backlog verdicts](../VERDICTS.md) · [Consent and keys](consent.md) · [Text checks](text-checks.md)

One verdict is one player's answer about one game. It is sent as JSON with exactly these fields; any other field refuses it.

| Field | Type | Values | Rules |
|---|---|---|---|
| `steam_appid` | integer | The game's Steam appid | At least one of `steam_appid` and `gog_id` |
| `gog_id` | string | The game's GOG product id, digits only | |
| `outcome` | string | `finished`, `playing`, `shelved`, `stopped`, `endless`, `bounced` | Required |
| `worth` | string | `yes`, `for_some`, `no` | Required |
| `hours` | string | `under_2`, `2_5`, `5_10`, `10_20`, `20_40`, `40_80`, `80_plus` | Never exact playtime |
| `clicked` | string | `first_hour`, `few_hours`, `later`, `much_later`, `never` | |
| `sessions` | string | `short`, `medium`, `long` | Review tags' `sessions` scale, value for value |
| `limited_time` | string | `yes`, `mostly`, `no` | |
| `factors` | list of strings | Helps: `save_anywhere`, `easy_to_resume`, `short_chunks`, `recap`. Hurts: `hard_to_resume`, `long_stretches`, `padding`, `obligations`, `needs_a_group` | Up to 3, each once, only with `limited_time` |
| `stop_point` | string | `at_credits`, `before_endgame`, `anytime`, `push_through` | |
| `stop_note` | string | Up to 80 characters ("after chapter 3") | Only with `stop_point`; the text checks below |
| `stopped_for` | list of strings | The game reasons UpNext asks for when a game is put down: `grind`, `stuck`, `lost_story`, `too_long`, `pacing`, `bugs`, `too_hard`, `boring` | Each once, only when `outcome` is `shelved`, `stopped` or `bounced`. Life reasons (`busy`, `other_game`, `mood`, `elsewhere`) are refused: they're about the person, not the game |
| `text` | string | Up to 500 characters | The text checks below |
| `vocab` | integer | The vocabulary the answers come from: `1` | |
| `licence` | string | `CC0-1.0` | |
| `consent` | integer | The consent version accepted: `1` | |
| `client` | string | The UpNext version that sent it, such as `0.1.0` | Letters, digits, `.`, `-` and `+`, up to 32. Never published |

Optional fields are left out rather than sent as `null`, though `null` reads as left out. Characters are Unicode scalar values (in JavaScript, `[...text].length`), not bytes. Every field's question, answers and words in the form are in [`verdicts-contract.json`](../verdicts-contract.json) under `fields`, with the "most players" line each answer reads as on the game page and, for `clicked` and `sessions`, the review tag it agrees with (`clicked` maps `first_hour` to `hook:fast`, `few_hours` to `hook:gradual`, `later` and `much_later` to `hook:slow`, and `never` to nothing).

**Never in a verdict:** a Steam ID or any store account, the person's name or player name, their own rating of the game, other games in their library, exact playtime, the play diary, the taste profile, any time finer than a day, the time zone, the locale, the computer's name, anything from settings, and life reasons for stopping.

## Pre-filled from the install

UpNext fills in what it can stand behind, and the player changes or adds the rest (`upnext verdicts draft <game>` shows it for a game):

- `outcome` from the player's mark: Beat it is `finished`, Shelved and Burnt out are `shelved`, Abandoned is `stopped`, Still playing is `endless`, Not for me is `bounced`. An unmarked game being played now is `playing`. UpNext's own guesses (a game it found shelved or abandoned from playtime and dates) aren't filled in.
- `hours` from the title's playtime, as its range, when a store reports one.
- `sessions` from the play diary, when it's on and holds at least five sessions of the game: the median session (play in back-to-back hours is one session), up to an hour `short`, up to two `medium`, longer `long`.
- `stop_point` is `anytime` for a game with no ending.
- `stopped_for` from the game reasons the player gave for putting it down, newest first, back to the last time every reason was cleared. Life reasons and the note typed with them stay on the install.
- `worth` and the rest are always asked.
