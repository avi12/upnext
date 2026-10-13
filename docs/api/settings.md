# Settings

[API index](../API.md) · [The server](basics.md) · [Players](players.md)

App settings live in the database and override `.env`, which overrides the built-in defaults. Keys and secrets never come back from any route, only whether they're set.

## GET /api/settings

Every in-app setting, with where each comes from:

```json
{
  "steam_api_key_set": true, "steam_api_key_source": "app",
  "steam_id": "76561197960287930", "steam_id_source": "steam client",
  "steam_persona": "player", "steam_avatar": "https://.../avatar_full.jpg",
  "model": "sonnet", "model_source": "default", "models": ["opus", "sonnet", "default"],
  "sampled_minutes": 120, "max_candidates": 120,
  "theme": "arcade", "themes": ["arcade", "ember", "daylight"], "theme_follow_system": false,
  "auto_sync": true, "last_sync": 1790000000,
  "heroic_dir": "/home/you/.config/heroic", "steam_local_dir": "/home/you/.local/share/Steam",
  "galaxy_db": null, "galaxy_db_exists": false,
  "gog": {"connected": false, "...": "..."}, "env_file": true, "os": "linux",
  "player_keys": ["model", "theme", "..."]
}
```

- `*_source` is `app` (set in the UI or the command line), `env` (from the environment or `.env`), `steam client` (a SteamID read from the client), `default`, or null when unset.
- `steam_persona` and `steam_avatar` come from the last Steam test or sync, and may be null.
- `theme` is `arcade` (the default), `ember` or `daylight`; `theme_follow_system` uses Daylight while the system is in light mode. The UI caches the theme in the browser so it paints before the page loads.
- `auto_sync` switches every [schedule](schedules.md); `last_sync` is in Unix seconds.
- `os` is the server's system (`linux`, `windows` or `macos`), so hints name the right folders and launchers.
- `player_keys` lists the settings that are each player's own (their preferences, their connections' and their AI account's). The rest are the household's: backups, schedules, network, folders, and whether the play diary runs.

The rest are on the pages they belong to:

| Settings | Page |
|---|---|
| `steam_api_key`, `steam_id`, `steam_include_hidden` | [Steam](steam.md) |
| `galaxy_db`, `galaxy_skip`, `galaxy_integrations`, `epic`, `epic_launcher_dir` | [More stores](more-stores.md) |
| `llm_backend`, `digest_backend`, the models, keys, addresses and efforts of each AI | [AI backends](ai-backends.md) |
| `igdb_client_id`, `igdb_client_secret`, `rawg_api_key`, `linux_mode` | [Game details](details.md) |
| `shelve_after_days`, `abandon_after_days`, `staple_hours`, `burnout_days` | [Statuses](statuses.md#settings) |
| `pattern_half_life_days`, `pattern_min_games`, `play_diary` | [Taste sources](taste-sources.md#settings), [Play diary](diary.md) |
| `gem_*`, `new_to_you_max_hours`, `unfamiliar_share`, `wishlist` | [Hidden gems](gems.md), [Discovery](discovery.md), [Wishlist](wishlist.md) |
| `verdicts_url`, `verdicts_ask`, `verdicts_receive`, `verdicts_prompt` | [Backlog verdicts](verdicts.md#settings), [from others](community-verdicts.md#jobs-and-settings) |
| `allow_other_devices`, `tls_cert`, `tls_key`, `listen`, `announce`, `network_name` | [Other devices](network.md) |
| `public_url`, `trusted_proxies`, `trusted_proxy_login` | [Proxies](proxies.md) |
| `oidc_*` | [Identity provider](oidc.md#settings) |

### The app window and the tray

- `window_size` (`"1280x860"`, logical pixels, not maximized; null until the window has closed once), `window_maximized` and `window_zoom` (percent): `upnext app` reads them when its window opens and saves them when it closes. `window_size` takes `WIDTHxHEIGHT`, each 200 to 16384, and `window_zoom` 30 to 300.
- `keep_running` (`true`) and `start_with_windows` (`false`), which only Windows acts on. With `keep_running`, closing the window leaves the server running, with the tray icon to open it again. `start_with_windows` starts the app at sign-in with only the tray icon: a `PUT` of it first writes or removes the `UpNext` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (`"<folder>\upnext-app.exe" --background`), and answers `400`, saving nothing, when it can't (no `upnext-app.exe` beside the running program). The value is shared by every copy of UpNext on the computer: turning the setting off leaves a value that names another copy still on disk as it is. Elsewhere both are only stored.

## PUT /api/settings

Any of the settings, by name: `{"model": "opus", "sampled_minutes": 120}`. `""` or `null` clears the app's value, so it falls back to `.env`, the client or the default. Every value is checked first, so a bad one saves nothing. Answers in the shape of `GET`.

- `400` for an unknown setting or a value it doesn't take, with the reason, such as a SteamID that isn't 17 digits or that another player's Steam connection has.
- `403` from another device for the settings that name files (`galaxy_db`, `tls_cert`, `tls_key`), change how UpNext runs on its computer (`start_with_windows`, `keep_running`), decide who gets in (the sign-in settings) or say where keys go (`verdicts_url`). See [Only on this computer](local-only.md).
- From another device, a new `openai_base_url` clears the saved `openai_api_key` unless the same request sends one, so a token can't send the key to an address of its choosing; with the key in the environment, such a change is `403` unless it brings its own key.
- A token bound to one player changes only that player's own settings (`403` for the household's).
