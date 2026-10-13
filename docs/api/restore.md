# Restoring and importing

[API index](../API.md) · [Backups](backups.md) · [Jobs](jobs.md)

Both answer only on this computer, whatever the token ([Only on this computer](local-only.md)).

## POST /api/backups/{name}/restore

A `restore` `Job` (`202`) that:

1. copies the backup into the folder and checks the copy (`quick_check`, and UpNext's `titles` and `games` tables);
2. backs up the database as it is (`restore` kind);
3. upgrades the copy to this build's schema, keeps this install's keys and sign-ins where the copy has none (the secret keys, `steam_id` with the Steam profile, and each player's GOG and Epic sign-ins), and puts this install's API tokens in place of the copy's, whatever either has, so a token revoked since stays revoked;
4. puts the copy in place with SQLite's backup API, on the server's own connection, so the server keeps running and every page shows the restored library at once.

Everything else is the backup's: library, statuses, lists, picks, settings, schedules, AI usage and job history. The restore keeps its own row and log under its id. Jobs the backup caught running are closed as `failed`, and scheduled jobs the restored schedules make due start as usual. The summary reads "Restored upnext-manual-20261001-101500.db, upgraded from v36; the database before it is upnext-pre-restore-20261002-090000.db".

`400` when the file isn't a database, or is from a newer UpNext or from before the first release; `404` for an unknown name; `409` while a job runs.

## POST /api/backups/import?name=old-laptop.db

The database file as the request's body (any content type, up to 2 GB). An `import` `Job` (`202`) that does what a restore does, with an `import` backup first. The file is checked before the job starts, so one UpNext can't use is a `400` with a plain reason ("that file isn't a database", "that database is from a newer UpNext", "that database is damaged", "that's a SQLite database, but not UpNext's", "the upload was empty; choose a database file"). `409` while a job runs.

## Opening an older database

When the server or the command line opens a database with an older schema that holds a library, it backs it up first; if that fails, the database isn't upgraded and doesn't open. A database from a newer UpNext isn't opened at all ("… is from a newer UpNext (schema vN); this build reads up to vM, so update UpNext"), and nor is one from a development build made before the first release, below schema v29, which can't be upgraded ("… move it aside to start a new library").

