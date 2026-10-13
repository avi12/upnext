# Backups

[API index](../API.md) · [Restoring and importing](restore.md) · [Only on this computer](local-only.md)

Settings › Backups. Backups are files in the `backups` folder next to the database (`~/.local/share/upnext/backups` on Linux), readable only by you. Each is a consistent copy (`VACUUM INTO`), checked with `PRAGMA quick_check` before older ones are removed, and why it was made is in its name:

| Kind | Name | Made by | Kept |
|---|---|---|---|
| `scheduled` | `upnext-scheduled-YYYYMMDD-HHMMSS.db` | the backup job (Jobs; `upnext backup`) | its newest `keep` (4) |
| `manual` | `upnext-manual-YYYYMMDD-HHMMSS.db` | Back up now; `upnext backup --note "…"` | until deleted |
| `upgrade` | `upnext-pre-upgrade-v37-YYYYMMDD-HHMMSS.db` | opening a database with an older schema | the newest 5 |
| `restore` | `upnext-pre-restore-YYYYMMDD-HHMMSS.db` | a restore, before it replaces the database | the newest 5 |
| `import` | `upnext-pre-import-YYYYMMDD-HHMMSS.db` | an import, before it replaces the database | the newest 5 |

Any other `*.db` in the folder counts as `manual`, and UpNext never removes it. Notes and reasons live in `backups.json` in the folder, not in the database, since a restore replaces the database. In a backup job's copy, its own job is marked done with the file's name as its summary, so restoring it doesn't show a backup that never ended. A second backup in the same second is named `…_2.db`.

## GET /api/backups

```json
{"folder": "/home/you/.local/share/upnext/backups", "schema": 38, "oldest": 29, "keep_scheduled": 4, "keep_automatic": 5,
 "backups": [{"name": "upnext-manual-20261001-101500.db", "kind": "manual", "made_at": 1790900100, "bytes": 38363136,
              "schema": 36, "note": "before trying the new prompt"},
             {"name": "upnext-pre-restore-20261002-090000.db", "kind": "restore", "made_at": 1790980000, "bytes": 38363136,
              "schema": 38, "reason": "Before restoring upnext-manual-20261001-101500.db"}]}
```

Newest first. `made_at` is the file's time. `schema` is read from the file's header, or null when it isn't a database. A backup whose `schema` is above the top-level `schema` (this build's), or from 1 to below `oldest`, can't be restored. A `schema` of 0 is a database with no schema number: a restore refuses one that holds a library at once, and any other in its first step. `note` is the note given with it and `reason` why UpNext made it on its own; each is absent when there's none.

## POST /api/backups

`{"note": "before trying the new prompt"}` (body and note optional, up to 300 characters) answers `201` with a new `manual` backup. It's made at once, even while a job runs, on a connection of its own.

## GET /api/backups/{name}/download

The backup without keys and sign-ins (`upnext-manual-…-without-keys.db`), safe to share or attach to a bug report. Left out:

- the API keys and secrets, `steam_id` and the Steam profile, the players' names, PINs and identity provider accounts, and store accounts' own names and ids;
- every setting that names a file or folder (the Galaxy database, a Heroic or Epic folder, the certificate and key);
- the household's addresses (`public_url`, `network_name`, `trusted_proxies`, `oidc_issuer`, `oidc_client_id`, `oidc_name`, `oidc_allowed_groups`, `verdicts_url` and each AI account's `openai_base_url`);
- the jobs' logs, and the jobs' results and errors, the AI calls' errors and the last errors kept in `meta`, each replaced by "(left out of a copy without keys)" so what failed still reads as failed.

Restored or imported on this install, its own settings come back where the copy has none; the players' identity provider links and other players' connection folders don't. `?keys=true` gives the file as it is, keys and sign-ins included, the GOG and Epic sign-ins among them. `404` for a name that isn't a backup in the folder.

`GET /api/export` gives the database as it is now, without keys and sign-ins (`upnext-YYYY-MM-DD-HHMM-without-keys.db`), by the same rule.

Restoring a backup and importing a database are on [Restoring and importing](restore.md).

## DELETE /api/backups/{name}

`204`, removing the file and its note. `404` for an unknown name.

Download, restore, import and delete answer only on this computer, whatever the token, and so does `keep` ([Only on this computer](local-only.md)). That holds behind a proxy on this computer too: the rule is who asks, not where the connection comes from.
