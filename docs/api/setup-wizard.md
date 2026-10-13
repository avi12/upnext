# What the wizard reads

[API index](../API.md) · [First-run setup](setup.md) · [Background work and uploads](setup-background.md)

## GET /api/setup

What every step shows, read fresh each time, with no AI or network call.

```json
{"environment": {"os": "linux", "mode": "serve", "container": false, "flatpak": null, "display": true, "gog_window": true, "client_local": true},
 "stores": {"libraries": [{"id": "steam", "label": "Steam", "blocked": null}, {"id": "gog", "label": "GOG account", "blocked": "not connected"}],
   "steam": {"path": "/home/you/.local/share/Steam", "missing": null, "error": null, "account": "76561197960287930",
             "account_source": "steam client", "persona": "Alex", "local_games": 812, "web_api": false,
             "local_accounts": [{"steam_id": "76561197960287930", "persona": "Alex", "player": "Alex"},
                                {"steam_id": null, "persona": null, "player": "Sam"}],
             "other_users": []},
   "heroic": {"path": "/home/you/.config/heroic", "missing": null, "error": null, "games": {"amazon": 1, "epic": 5, "gog": 2},
              "owner": "Alex", "other_users": ["/home/sam/.config/heroic"]},
   "gog": {"connected": false, "username": null}, "galaxy": {"path": null, "exists": false},
   "epic": {"connected": false, "display_name": null, "launcher_dir": null, "installed": null},
   "ea": {"installed": 0}, "ubisoft": {"installed": 0}},
 "keys": {"steam_api_key": false, "steam_id": true, "rawg": false, "igdb": false},
 "ai": {"backend": "claude-cli", "available": true, "error": null, "claude": "/home/you/.local/bin/claude", "claude_install": null, "claude_token": false},
 "library": {"titles": 99, "copies": 99, "multi_store": 0, "by_source": {"steam": 95, "gog": 2, "epic": 2, "amazon": 0},
   "enriched": 99, "unenriched": 0, "details_minutes": 0.0, "last_sync": 1790986302, "job": null},
 "setup": {"needed": false, "step": "done", "...": "..."},
 "you": {"id": 1, "name": "Player 1", "placeholder": true, "suggested": "Alex"},
 "background": {"sync_waiting": false, "details_waiting": false, "seed": {"wanted": true, "state": "applied", "written": 412, "error": null}}}
```

## environment

- `mode`: `app` (the desktop app), `service` (the systemd unit sets `UPNEXT_MODE=service`) or `serve`. `container` is true in the container image.
- `flatpak` is the Flatpak's app id (`io.github.zytocc.UpNext`) inside one, else null, so the wizard can say how to let the sandbox read a library elsewhere and how to install Claude Code inside it.
- `display`: the server's computer has a screen to open a window on (never in a container), checked rather than read from the variables. It's always true inside `upnext app`. On Linux it's a Wayland or X socket that takes a connection, or a `wayland-N` socket in the server's own `XDG_RUNTIME_DIR`, which a service started before the desktop finds this way; a display with a host, as from `ssh -X`, counts. On Windows it's false for a service account and for `UPNEXT_MODE=service` without `SESSIONNAME`. On macOS it's true. `gog_window` says whether the [GOG sign-in window](gog.md#the-sign-in-window) can open.
- `client_local`: the browser is on the server's computer, so paths and windows mean something to the reader.

## stores

- `libraries` is [`GET /api/sync/sources`](sources.md).
- `steam` and `heroic` give the folder found or set, else `missing` (`not found`, or `not mounted` in a container), and `error` when it was found but couldn't be read. Heroic's `games` counts each store's cache.
- Steam's `account` is the SteamID in use or read from the client, with `account_source`, and `web_api` says whether the key and SteamID are both set. `local_games` is what the client's caches list for that account before any sync: only the games it played or installed when it isn't the account signed in to Steam last ([Steam](steam.md#what-a-sync-reads)).
- `steam.local_accounts` lists every account signed in to this computer's Steam client, most recent first, with `persona` and `player` (whose Steam connection has it, or null). Another player's account shows only `player`. For the first player, `account` is the client's most recent account; for anyone else it's their own SteamID, else the first local account nobody has, which their setup offers them ("That's me").
- `heroic.owner` is the player whose Heroic connection reads the folder shown. Heroic has one sign-in per user of the computer, so another player's Heroic is a folder of their own (`heroic_dir`); one with none is shown the first player's, so their setup can say whose it is.
- `other_users` lists the folders found in other users' home folders on this computer, for a request from this computer and never in a container: Steam's when this computer's own isn't found (`UPNEXT_STEAM_DIR` then names the one to read), and Heroic's that no player reads yet. Their setup offers each ("That's mine"), checking only that the files are there.

## The rest

- `keys`: whether each is set. The values never appear.
- `ai`: the chosen backend and whether it can run, with no call. `claude` is where the Claude CLI was found, or null. `claude_install` is a container's install of Claude Code (`UPNEXT_INSTALL_CLAUDE=1`): `{"installing": true, "error": null}` while it runs, `{"installing": false, "error": "…"}` when it failed, and null otherwise. `claude_token` says whether `CLAUDE_CODE_OAUTH_TOKEN` is set for the server, never its value.
- `library`: the counts, `details_minutes` (an estimate for the titles without details, 3.05 seconds each), the last sync and the running job.
- `setup` is [`GET /api/setup/state`](setup.md#get-apisetupstate), and `background` is what [`POST /api/setup/background`](setup-background.md) answers.
- `you` is the request's player, for "Who plays here?". `placeholder` is true while their name is one UpNext made up ("Player 1", "Player N"). `suggested` is then a name to offer: the Steam account's `persona`, else the first player's Steam profile name, else (from this computer only, never in a container) the user name UpNext runs under, with a capital, but not root, a service account or a shared login such as `deck`, `steam`, `user` or `admin`. It's null when they have a name of their own, nothing fits, or another player has it. A player's name is never sent to the AI, the verdicts service or anywhere else.
