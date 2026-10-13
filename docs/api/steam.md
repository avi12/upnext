# Steam

[API index](../API.md) · [GOG](gog.md) · [More stores](more-stores.md) · [Settings](settings.md)

The Steam Web API key (`steam_api_key`, write-only) and the SteamID (`steam_id`, 17 digits) are [settings](settings.md). Without a SteamID set, UpNext reads the one most recently signed in to the local Steam client (`steam_id_source` is then `steam client`). A SteamID belongs to whoever linked it: one another player's Steam connection has already is a `400`, naming whose it is ([Accounts](accounts.md)).

## POST /api/steam/test

Calls Steam with the key and SteamID in use. `{"ok": true, "persona_name": "player", "avatar": "...", "game_count": 812}`, or `400` `{"error": "..."}` (a bad key, a private profile and so on). The name and picture are kept and shown as `steam_persona` and `steam_avatar`.

## Sign in with Steam

Signing in with Steam (OpenID) gives the SteamID, never the API key, which Settings still asks for.

1. The UI sends the browser to `/api/steam/login`. The server answers with a redirect to Steam's OpenID page and a one-time state, good for ten minutes, tied to this browser by an `upnext_steam_signin` cookie (`SameSite=Lax`, so the way back from Steam carries it). So the sign-in has to come back to the browser that opened the address, and a script can't start one for a browser. At most 16 sign-ins wait at once; a seventeenth pushes out the oldest. It needs a `full` token, a paired browser or this computer, since the sign-in saves the SteamID.
2. Steam sends the browser back to `/api/steam/callback`, which needs no token: Steam sends the browser from its own site, so a paired browser's `SameSite=Strict` cookie doesn't come with it. The callback takes only that state, once, from the browser it was given to, and clears the cookie. The server checks the answer with Steam, stores the SteamID, and sends the browser to `/settings?steam=ok`, or `/settings?steam=error&reason=...`. A SteamID another player's connection has is refused there too. The Settings page reads those parameters once, shows a toast and cleans the address.
3. `/api/steam/login?then=setup` comes back to `/setup?steam=ok` (or `…error…`) instead, so a sign-in from the wizard returns to it, and `then=player` to `/setup/player?steam=…`, a player's own setup. Any other `then` is ignored.

Where Steam sends the browser back to is in [public_url and proxies](proxies.md#public_url).

## What a sync reads

With the key, a sync reads the owned games and playtime from the Web API; without it, from the local Steam client's files, or both with a sync's `steam_local` option. The client's package cache holds only the licences of the account signed in to Steam last, so for any other account on the computer it reads only the games that account played or installed, and doesn't take the rest as removed.

`steam_include_hidden` (`false`): games hidden in the Steam client are left out, read from the local client's `userdata/<account>/config/cloudstorage/cloud-storage-namespace-1.json` (`user-collections.hidden`) and the older `sharedconfig.vdf` when either is there. With the Web API alone and no local client, nothing is known to be hidden. `true` syncs them like any other game.

Adding UpNext to Steam as a shortcut for big screen is [Steam shortcut](steam-shortcut.md).
