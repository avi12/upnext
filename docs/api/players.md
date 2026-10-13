# Players

[API index](../API.md) · [Accounts](accounts.md) · [Identity provider](oidc.md)

The people who play on one install, each with their own marks, picks, taste profile, ratings, routines, wishlist, settings and hidden games, sharing one record per game. A script that names no player acts for the default player.

## Who a request is for

1. **A token bound to a player** (`POST /api/tokens` with `player_id`, `upnext token create --player Sam`, or a device paired for them) acts for that player and no one else. An `UpNext-Player` header naming anyone else is `403`.
2. **The `UpNext-Player` header** names a player by id, for scripts. An id that isn't a number, or no such player, is `400`; a player with a PIN is `403` (a script acts for them with a token bound to them). Every operation that answers for one player lists the header in the OpenAPI document (`#/components/parameters/UpNextPlayer`).
3. **The player cookie** (`upnext_player_<port>`, `HttpOnly`, `SameSite=Strict`, `Secure` over HTTPS, signed by this install) that `PUT /api/players/current` sets: who's playing in this browser. One that isn't this install's, or names a removed player, is cleared and the request goes on to the default.
4. **The default player.** While the default player has a PIN and nobody is chosen, a request for one player is `403` "choose who's playing: the default player has a PIN"; the players, `GET /api/players/current` and `PUT /api/players/current` still answer.

A token or device bound to a player can't add, rename, recolour or remove players, switch to another, change the schedules or setup, or change a setting that is everyone's (`403`). Nor can it read `GET /api/export`, the whole database with every player's marks and hidden games; `GET /api/taste/export` is its own player's. It can change its own player's settings and PIN.

## GET /api/players

```json
{"players": [{"id": 1, "name": "Alex", "colour": "#4f8cff", "has_pin": false, "created_at": 1790000000, "default": true,
              "oidc": null, "oidc_link_until": null, "oidc_waiting": null}],
 "current": 1, "bound": false}
```

`current` is null while the default player has a PIN and nobody has been chosen. `oidc` is the issuer of the identity provider account linked to the player, or null; `oidc_link_until` is when "Link the next sign-in" ends (Unix seconds), or null; and `oidc_waiting` is the account waiting to be confirmed as them, `{"subject", "name", "username", "email"}` from its ID token, shown to tell who it is and never matched. Only this computer, which confirms it, is shown `oidc_waiting`; to anyone else it's null ([Identity provider](oidc.md#linking-an-account-to-a-player)).

## Adding and changing players

- `POST /api/players` `{"name": "Sam", "colour": "#eb5757"}` (`colour` optional) answers `201` with the player, who sees what is shared with everyone at once, without anyone else's hours. `400` for no name, a taken one or a bad colour.
- `PUT /api/players/{id}` `{"name", "colour", "default": true}`, each optional, answers with the player.
- `PUT /api/players/{id}/pin` `{"pin": "1234", "new_pin": "5678"}`, only as that player and with the PIN when they have one; `new_pin` null or `""` removes it. A PIN is four to eight digits (`400`). Answers `204`.
- `DELETE /api/players/{id}`, with `{"pin": "1234"}` when they have one, else `{}`, only on this computer. A backup first ("Before removing Sam"), then their marks, history, taste profile, lists, store connections with their accounts and games, AI accounts and bound tokens go. The others lose the games and AI that came only from those accounts. `{"backup": "upnext-manual-….db", "removed": "Sam"}`; `400` for the last player.

Five wrong PINs in a row lock that player's PIN for a minute (`429`), then two, four and so on, and the right one clears the count. The same count covers choosing the player, unlocking their hidden games, removing them, and binding a token or a device to them.

## GET and PUT /api/players/current

`GET` answers `{"player": <player> | null, "bound": false}`. `PUT` `{"player_id": 2, "pin": "1234"}` chooses who's playing in this browser and sets the player cookie; `403` for a wrong or missing PIN, `429` while locked.

Hiding a game from yourself or the others is on [Hiding games](hiding.md).

## Elsewhere

- **Settings** read and write the request's player's own settings, their connections' and their AI account's, and the household's ([Settings](settings.md)). Which is which is in `src/db/players.rs`.
- **Tokens and devices.** `POST /api/tokens` takes `player_id`. `POST /api/pairing?player=<id>|shared` makes a code for a device that is that player's, or shared; by default the player making the code when there are two or more players, and shared with one. Binding to a player with a PIN takes it (`"pin"` in the token's body, in a JSON body for `POST /api/pairing`, never in the address, and in the answer to a device asking): `403` when missing or wrong, and a wrong one counts towards the lockout.
- **Jobs** are the household's: sync, enrich and backups run with player 1's settings and the library's AI account. The taste patterns, the wishlist and Second Chance run once per player, each on that player's AI account.
- **AI usage** counts the calls this player sees: their own requests, and every call on an AI account they own. `by_player` adds them up by who asked (the library's jobs as `player_id: null`, "The library"), and `by_account` by the AI account that ran them, the player's own first. On an account shared with them a player sees only their own requests.
- **The command line**: `--player <name or id>` (or `UPNEXT_PLAYER`) before any command chooses who it acts for; `upnext player list|add|rename|remove|pin`.
