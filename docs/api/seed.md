# Shared game data

[API index](../API.md) · [Game details](details.md) · [Background work in setup](setup-background.md)

UpNext's shared data is People say, Vibes and review tags for popular games, written once by UpNext's maintainers and published under CC0 on UpNext's data server, `https://data.upnext.games`. New data is published there when it's rebuilt, not with each release of the app. Applying it gives matching titles their summaries with no review fetch and no AI call.

- Each title is matched by its Steam appid, then by the GOG id of any of its GOG copies.
- A title keeps its own summary when that was written with the same prompt version (`DIGEST_VERSION`) or a newer one; an older own summary, or older shared data, gives way.
- Applied data is kept beside the database (`seed/upnext-seed-<date>.jsonl.gz`), so games that join the library later get their summaries after each sync, before the summaries job asks the AI.
- A summary from shared data stays when a release raises `DIGEST_VERSION`, until newer data arrives: the summaries job and the review fetch skip seeded titles unless an enrich names them (`--titles`). Their player reviews aren't fetched until the game page asks ([`POST /api/games/{id}/reviews`](details.md#post-apigamesidreviews)).
- `GameView.seed_date` is the date of the shared data the title's summary, Vibes and review tags came from (YYYY-MM-DD), or null. The game page says "From UpNext's shared data, October 2026" under People say.

## The format

Version 1, in `src/seed.rs`. `upnext-seed-<date>.jsonl.gz` is gzipped JSON lines. Line 1 is the header, `{"upnext_seed": 1, "date": "2026-10-01", "count": 10000, "sha256": "…", "licence": "CC0-1.0", "notice": "…"}`, where `sha256` is of everything after the first line, uncompressed. Every other line is one game: `steam_appid` and/or `gog_id`, `name`, `people_say`, `vibes`, `tags` (the `review_tags` shape), `prompt_version`, `model`, `written` and `reviews_until` (dates, YYYY-MM-DD).

Any other field refuses the whole file: that's the allowlist (identifiers, the three AI-written texts, and how and when they were written). Tag values outside this build's vocabulary are dropped, as they are from the AI's replies.

Beside it on the server, `upnext-seed.json` is the manifest: `{"upnext_seed": 1, "date", "count", "licence", "notice", "file": "upnext-seed-<date>.jsonl.gz", "bytes", "sha256"}`, with the data file's size and SHA-256 as downloaded. A damaged file, a checksum or count that doesn't match, a newer format or another licence refuses the file before anything is written.

## GET /api/seed

```json
{"seeded": 812, "seed_date": "2026-10-01", "kept": "upnext-seed-2026-10-01.jsonl.gz", "applies": [SeedApply],
 "default_url": "https://data.upnext.games/upnext-seed.json"}
```

- `seeded`: titles whose summary came from shared data; `seed_date` the newest data's date among them.
- `kept`: the data file applied again after each sync, or null.
- `applies`: the latest five applies, newest first: `{"applied_at", "seed_date", "source", "entries", "matched", "written", "kept_own"}`. `source` is the address, the uploaded file's name, or "the kept shared data, after a sync" (recorded only when that wrote something).
- `default_url`: the newest published data's manifest, on UpNext's data server.

## POST /api/seed

`{"url": "https://…/upnext-seed.json"}` (a manifest or a data file), or nothing for `default_url`. Downloads it (up to 64 MB, following redirects only to https), checks it and applies it. Answers `SeedApplied`: `{"date", "entries", "matched", "written", "kept_own", "already"}`, where `already` counts titles that had this data's summary or a newer one's.

`400` for an address that isn't http(s), an answer that isn't 2xx, or a file UpNext won't apply, with why; `502` when the download fails. One download or upload is applied at a time. Applying replaces this install's own summaries written with an older prompt, so removing the data later leaves those titles without one until the summaries job writes them again.

## POST /api/seed/offer

`{"url": "https://…/upnext-seed.json"}`, or nothing for `default_url`. Reads the manifest without downloading the data and answers `SeedManifest`, `{"upnext_seed", "date", "count", "licence", "notice", "file", "bytes", "sha256"}`, for the wizard's "Use UpNext's game data (10,000 games, updated October 2026)". `400` and `502` as above.

It's a `POST` though it changes nothing, so another website can't make the server fetch an address with an image or a link, and it needs a `full` token (or this computer), since a read token could use it to probe the server's network. The server reads it, so the page works the same from another device. When the offer fails, the wizard leaves the option out.

## POST /api/seed/upload?name=upnext-seed-2026-10-01.jsonl.gz

The data file's bytes (`Content-Type: application/octet-stream`, up to 64 MB), checked against its own header's checksum. Answers `SeedApplied`, or `400` with why.

## DELETE /api/seed

Removes the summaries that came from shared data and the kept copy, so this install's own AI writes them on the next summaries run. Answers `{"removed": 812}`. The record of applies stays.

`upnext seed apply [file or address]`, `upnext seed status` and `upnext seed remove` do the same from a terminal.
