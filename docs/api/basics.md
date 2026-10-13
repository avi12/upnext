# The server, its profile and its numbers

[API index](../API.md) · [Settings](settings.md) · [The library](library.md)

## GET /api/health

```json
{"app": "upnext", "version": "0.1.0", "build": "abc1234", "container": false, "id": "4f2a9c01d3e85b67"}
```

Which program answers on the port, its version, and its build: the short commit it was built from, or `null` for a build made without git (a container build from a source tree without `.git`).

- `upnext app` asks it before opening a window on a port, to tell a running UpNext from another program; anything without this answer is another program. `upnext healthcheck`, the container's health check, asks it too and prints the version and build, as Settings › About shows them.
- The window compares `version` and `build` with its own. Another version, or the same version with another build or none, still gets the window, but every page shows a bar saying which build each is and to quit the other UpNext, since two servers on one library would run its scheduled jobs twice. A window built without git compares versions only.
- `container` is true in the container image (`UPNEXT_CONTAINER=1`). The pairing page, which has no token yet, reads it to say the code is in the container's log.
- `id` is the install id the network announcement carries. The app window on another computer checks it before sending that computer its token.

## GET /api/openapi.json

This API as an OpenAPI 3.1 document, made from the code, so it's the precise contract: every path (relative to `/api/v1`), its parameters, bodies, answers and schemas, and who may call it (`security`: a `read` or `full` token, or a paired browser; `x-local-only` for what answers only on this computer). A copy is in [`docs/openapi.json`](../openapi.json), and the tests fail when it differs from what the code serves (`UPNEXT_UPDATE_OPENAPI=1 cargo test openapi` writes it again).

## GET /api/profile

```json
{"steam_id": "76561197960287930", "persona_name": "player", "avatar": "https://.../avatar_full.jpg",
 "profile_url": "https://steamcommunity.com/id/...", "last_sync": 1758990000, "game_count": 1600,
 "sources": [
   {"source": "steam", "launcher": "steam", "label": "Steam", "configured": true, "count": 812, "last_sync": 1758990000},
   {"source": "epic", "launcher": "heroic", "label": "Epic", "configured": true, "count": 360, "last_sync": 1758990000, "note": "Heroic + Epic account"},
   {"source": "galaxy", "launcher": "galaxy", "label": "GOG Galaxy import", "configured": false, "count": 0, "last_sync": null}
 ]}
```

`persona_name`, `avatar`, `profile_url` and `last_sync` may be null before the first sync. `configured` is false when that launcher's credentials or files aren't there. `note`, when present, says which way a store's games come in ("Heroic + GOG account") and goes under the label. `game_count` counts titles, and `copy_count` store copies. A player other than the first gets the `steam_local` profile source only when their own SteamID is signed in here, and `heroic` only when their own folder is set.

## GET /api/config

What the server runs with:

```json
{"model": "sonnet", "sampled_minutes": 120, "max_candidates": 120,
 "db_path": "/home/you/.local/share/upnext/upnext.db",
 "claude_available": true, "llm_error": null, "llm_backend": "claude-cli",
 "steam_configured": true, "steam_api_configured": true, "steam_local_dir": "/home/you/.local/share/Steam",
 "steam_id_source": "steam client", "gog_connected": false, "epic_connected": false,
 "heroic_config_dir": "/home/you/.config/heroic", "galaxy_db": null,
 "linux_active": true, "rawg_configured": false}
```

`claude_available` says whether the AI in use can run, whatever the backend, and `llm_error` why not. `steam_configured` is true with a Steam API key or a local Steam client; `steam_api_configured` only with the key.

## GET /api/stats

```json
{
  "total": 812,
  "by_status": {"untouched": 410, "sampled": 120, "playing": 6, "shelved": 31, "played": 130, "done": 30,
                "still_playing": 2, "burnt_out": 1, "abandoned": 40, "hidden": 5, "not_game": 35},
  "by_source": {"steam": 812, "epic": 360, "gog": 300, "amazon": 146},
  "installed": 40, "candidates": 530, "staples": 11,
  "enriched": 780, "unenriched": 32, "with_reviews": 770, "with_tags": 700, "with_hltb": 300,
  "total_playtime_minutes": 512000,
  "genres": [{"name": "Action", "count": 300}],
  "tags": [{"name": "Indie", "count": 400}],
  "top_played": ["GameView", "..."]
}
```

`genres` and `tags` are sorted by count, most first. `top_played` is the ten most-played games.

## Logs

`GET /api/logs?level=warn` gives the server's recent warnings and errors (the last 300), oldest first: `[{"at": 1758990600, "level": "warn", "message": "…", "job_id": 42}]`. `job_id` is null outside a job, `level=error` gives errors only, and an unknown `level` is `400`. A job's own log is under [Jobs](jobs.md).

## The UI

Everything not under `/api/` serves the built UI from `web/dist`, embedded in the binary, with `index.html` for the UI's own routes. A path under `/api/` that no route matches answers `404 {"error": "no such API route: /api/v1/…"}`, and a known path with a method it doesn't take answers `405`.
