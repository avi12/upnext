# Background work and uploads in setup

[API index](../API.md) · [First-run setup](setup.md) · [What the wizard reads](setup-wizard.md)

## POST /api/setup/background

Any of `{"sync": "if_changed", "details": "wait", "seed": true}`. Answers `{"sync_waiting": false, "details_waiting": false, "seed": {"wanted": true, "state": "waiting", "written": null, "error": null}}`.

The wizard runs the first sync and Fetch details in the background while the person carries on. One task on the server starts each as the job slot frees, in this order: the shared data after the first sync, a sync, then Fetch details. Each starts as the scheduled task of the same name, with its own options, from `ui`. It's the household's, so it takes no `UpNext-Player` header, and it needs a `full` token (`403` for a read token).

- `sync`: `if_changed`, sent when the person leaves the store steps, syncs unless the wizard's last sync read the same accounts and folders (Steam's SteamID, key and folder, Heroic's folder, the Galaxy file and what it skips, hidden Steam games, the GOG and Epic sign-ins, and which libraries are blocked) and is running, waiting or brought games. `again`, from the First sync step's button, syncs anyway. While a job runs, the sync waits (`sync_waiting`); when that job is a Fetch details the wizard started, it's stopped first and starts again after the sync, carrying on where it stopped.
- `details`: `wait`, sent with the sync from the store steps, holds the schedules (no task starts by itself) on a first run only, until the wizard asks for details, setup ends, or an hour passes, so Fetch details doesn't start before the data-source keys of the next step are in. After a first run, and on a wizard restarted from Settings, it's ignored. `after_sync`, sent when the person leaves that step, fetches details once the sync ends (`details_waiting`), or at once when no sync runs or waits; nothing starts when every game has its details.
- `seed`: [shared data](seed.md) is applied when the wizard's first sync ends with games, whatever page is open by then, unless `seed` is `false`. `seed.state` is `idle` (no sync asked yet), `waiting`, `applying`, `applied` (with `written`, the summaries written), `failed` (with `error`) or `skipped`: the library had games already, shared data is kept already, it was turned off, or the newest data's manifest couldn't be read (none is published yet, or the data server can't be reached), which isn't an error.

What was asked lives in the server's memory: a restart forgets it, and the schedules carry on as usual. If the server can't read what it needs twenty times in a row, the background work stops with a warning in the log, and the next ask starts it again.

## POST /api/galaxy/upload

The bytes of a `galaxy-2.0.db` (`Content-Type: application/octet-stream`), for a Galaxy database copied from another computer. Up to 1 GB (`413` over that). The file is checked as a Galaxy database, saved as `galaxy-2.0.db` next to UpNext's database, and `galaxy_db` is set to it. Answers `{"path": "/home/you/.local/share/upnext/galaxy-2.0.db", "games": 212}`.

- `400` with the reason for an empty file or one that isn't a Galaxy database (`"that isn't a GOG Galaxy database: file is not a database"`); nothing is kept.
- `409` when another upload has waited 30 seconds for this one, or when the kept file is still in use after five tries over 1.6 seconds (on Windows, a sync reading it holds it): `"the Galaxy database UpNext keeps is in use, most likely by a sync reading it; try again once the sync is done"`.
- `408` for an upload that sends nothing for a minute or hasn't finished after 15 minutes; nothing is kept.
- `500` naming the folder for any other failure to save it, such as a full disk, with the system's error in the log.

A `full` token can upload, since the file is checked and the path is UpNext's own, unlike typing a `galaxy_db` path, which only this computer may.

## From another device or a container

The wizard works through a pairing like the rest of the UI. `GET /api/setup` takes any token; `PUT /api/setup`, `PUT /api/settings`, the Galaxy upload and the jobs take a `full` one, as a paired browser's cookie is. `environment.client_local` is false.

What stays on the server's computer: the GOG sign-in window (the ["Send to UpNext" bookmark](gog.md#the-send-to-upnext-bookmark) stands in for it), file paths in `PUT /api/settings`, and importing a database. Steam's sign-in comes back through the open `GET /api/steam/callback` ([Steam](steam.md#sign-in-with-steam)). In a container, the "Where UpNext runs" step builds the `docker run` and compose lines for the Steam and Heroic folders from what the person picks, since the server can't see the host's folders; they match `contrib/docker/compose.yaml`'s commented lines.
