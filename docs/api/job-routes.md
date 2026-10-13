# Starting and following jobs

[API index](../API.md) · [Jobs](jobs.md) · [Scheduled jobs](schedules.md)

Each route answers with the `Job` that [Jobs](jobs.md) describes.

## GET /api/jobs

`{"current": Job | null, "recent": [Job]}`: the running job, from either process, and the finished ones, newest first. Every parameter is optional: `limit` (20 unless asked, up to 200), `kind`, `task` (a task's id, or `none` for jobs that weren't one) and `failed=true` (only jobs that failed or finished with failures). `400` for an unknown kind or task.

## POST /api/jobs

- `{"task": "reviews"}` runs a scheduled task by hand, with its own options (Run now). Add `"refresh": true` to fetch again what titles already have.
- Or a kind and options: `{"kind": "sync", "options": {"steam": true, "galaxy_db": null}}` (every configured library unless told), `{"kind": "enrich", "options": {"hltb": true, "refresh": false, "limit": null}}`, `{"kind": "backup", "options": {"keep": 4}}` or `{"kind": "taste"}`. Such a job counts as the task whose work it does.

Answers `202` with the `Job`. `400` for a task together with a kind or options. `403` from another device for a sync's own `galaxy_db` other than the one set, or a backup's `keep`. `409` while a job runs, saying which: `{"error": "an enrich started from a terminal 5 min ago is running (1,040 of 1,600); wait for it or cancel it"}`.

## POST /api/jobs/current/cancel

Answers with the `Job`, `cancel_requested: true`. It stops at its next check, about a second, and ends `cancelled`; an AI call still running is cut off, though a provider that charges per call may still charge for it. It works for a job running in a terminal too. `404` when none is running.

## GET /api/jobs/{id}

`?after=0&level=warn&limit=500`, all optional. The `Job` plus:

- `failures`: `[{"lane": "Steam reviews", "source": "Steam reviews", "options": ["reviews", "review_texts"], "title_id": 1234, "name": "Portal 2", "error": "HTTP 429 Too Many Requests for https://store.steampowered.com/appreviews/620", "at": 1758990600}]`. `options` are the enrich options that fetch what failed (empty for the Steam store step, which always runs). A pass's failure has no `title_id`, and an AI series guide's `name` is the series. A title an AI summaries reply left out is a failure too, with source `digests`.
- `can_retry`: whether `retry-failed` has anything to run.
- `log`: `[{"id": 881, "at": 1758990600, "level": "warn", "lane": "Steam reviews", "message": "Steam asked us to slow down, trying again in 40 s (…, try 4 of 5)"}]`, oldest first, with `log_more` when more lines follow. `level` is `info`, `warn` or `error`, and warnings and errors that UpNext's own code logs during the job land here too. Page it with `after` (a line's `id`), `level=warn` (warnings and errors) or `error`, and `limit` (up to 5,000).

`404` for an unknown id. A game hidden from the player asking reads "a hidden game" in every job route and in `GET /api/logs`.

## GET /api/jobs/{id}/log.txt

The whole log as a file (`upnext-enrich-42.log`), a line each: local time, level, `[lane]`, message.

## POST /api/jobs/{id}/retry-failed

A new enrich `Job` (`202`) that runs the sources that failed, for the titles they failed on only, with the job's `refresh`. `400` when nothing can run again (no failure has a title); `409` while that job or another runs.
