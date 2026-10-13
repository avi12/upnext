# Jobs

[API index](../API.md) · [Starting and following jobs](job-routes.md) · [Scheduled jobs](schedules.md)

A job is background work: a sync, an enrich, a backup, a restore, an import, a taste profile count or a download of other players' verdicts. It runs in the server (started here, or by its schedule) or in a terminal (`upnext sync`, `upnext enrich`, `upnext backup`). Either way it's recorded in the database, which the process running it updates every second, so the API shows both and one runs at a time across them. A running job whose process stops writing for 30 seconds (closed, killed or crashed) is marked `failed`. The last 50 jobs are kept with their logs, and so are each scheduled task's last run and last run that ended done, however old.

## The Job

```json
{"id": 42, "kind": "enrich", "task": "details", "source": "terminal", "status": "running",
 "started_at": 1758990000, "finished_at": null, "heartbeat_at": 1758990630,
 "total": 1600, "done": 1040, "current": null, "message": "in parallel: Steam store 1040 · Steam reviews 1038 · …",
 "per_minute": 31.2, "eta_secs": 1010, "failed": 3, "summary": null, "error": null, "cancel_requested": false,
 "lanes": [{"name": "Steam reviews", "done": 1038, "total": 1600, "failed": 2, "state": "waiting", "current": "Portal 2",
            "note": "Steam asked us to slow down", "retry_at": 1758990670}],
 "options": {"hltb": false, "reviews": true, "tags": true, "refresh": false, "limit": null},
 "ai_usage": null}
```

- `kind`: `sync`, `enrich`, `backup`, `restore`, `import`, `taste` or `verdicts`. A restore or an import starts only from [Backups](backups.md).
- `task`: the [scheduled task](schedules.md) it ran as or did the work of, or null for other work, such as a retry of a few titles or a single source.
- `source`: `ui`, `schedule` or `terminal`.
- `status`: `running`, `done`, `failed` or `cancelled`. `total` may be 0 while unknown. `summary` says what a finished job did ("2,000 games, 3 new"; "1,600 titles: 3 matched, 1,500 reviews, 5 errors").
- **A partial sync.** When an account (a player's Steam account, Heroic folder, Galaxy database, Epic or GOG sign-in) fails while the rest sync, the job is `done` but its summary starts "Partial:" and names what failed ("Partial: 2,000 games, 3 new; Steam (Sam) failed"). Each failure is in `failed` and `failures`, with lane `sync`, the account as `source` and no `title_id`. A library that failed before reaching an account is one failure under its own name. A sync where something failed and nothing was read is `failed`, and its `error` names each account and why ("Steam (Sam) failed: …; Heroic failed: …").
- `per_minute` and `eta_secs`: the rate over the last few minutes and the time left at that rate; null until there are 20 seconds and 3 titles to go on.
- `lanes` (enrich only), in the order they started. One per rate-limited service taking every title (`Steam store`, `Steam reviews`, `SteamSpy`, `ProtonDB`, `GOG`, `IGDB`, `RAWG`, `PCGamingWiki`, `HowLongToBeat`) and one per pass over the library (`Wikidata`, `Series covers`, `Steam art`, `IGDB trailers`, `AI review summaries`, `AI series guides`). `state` is `running`, `waiting` (a service asked it to slow down, failed or didn't answer; `note` says which and `retry_at` when it tries again), `stopped` (it gave up for the rest of the run: a rejected key, too many failures in a row, a usage limit; `note` says why) or `done`. While lanes run, the job's `done` is the slowest lane's, and the AI lanes count each call as it comes back.
- `failed`: failures so far, one per title and source.
- `ai_usage`: null unless the job made AI calls. Otherwise the calls' totals ([AI usage](ai-usage.md)) plus `estimated_tokens`: while the job runs, the tokens the whole job should take, from its own average so far and what its AI lanes have left (null until a call has come back for each AI lane with work left, and once the job ends).
- `options`: for an enrich, every source with the value it ran with ([Sync and enrich sources](sources.md#enrich-options)). For a sync, one boolean per library (`{"steam": true, "heroic": true, "gog": false, "galaxy": true}`, keyed as `GET /api/sync/sources` lists them), plus `steam_local` (read the local Steam client even with an API key) and `galaxy_db` (a `galaxy-2.0.db` path; Galaxy is skipped without one); a library left out is on and an unknown key is ignored. For a backup, `{"keep": 4}`; for a restore, `{"name": "upnext-manual-20261001-101500.db"}`; for an import, `{"name": "old-laptop.db", "staged": "…/backups/.work-….db"}`.

[Starting and following jobs](job-routes.md) has the routes.
