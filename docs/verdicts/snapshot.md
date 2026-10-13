# The published snapshot

[Backlog verdicts](../VERDICTS.md) · [Reading it](reading.md) · [A verdict](verdict.md)

The service builds a snapshot once a day, under `verdicts_url`, in three files. Each is complete, never a change to the one before, so a withdrawn or removed verdict never lingers in an install that keeps syncing.

- **`verdicts.json`**, the manifest, under a stable name. Written last, so a reader never sees a manifest for a half-written file. Served with a short cache lifetime and an `ETag`, so an unchanged day costs one small request with `If-None-Match`.
- **`verdicts-summary-<date>.jsonl.gz`**: the games with at least 3 verdicts. This is what installs download, always whole and never a game at a time, so the host never learns which games anyone owns.
- **`verdicts-all-<date>.jsonl.gz`**: every published verdict, for anyone who wants the raw data. Installs don't download it.

The dated files are kept for six days and served with a cache lifetime of at most a day, so a withdrawn verdict is gone from UpNext's servers within a week, as the consent screen says.

Both data files are gzip'd JSON lines. The first line is a header; the `sha256` in it is of everything after the first line, uncompressed, so a file checks itself wherever it came from. Months (`YYYY-MM`) are the finest time in any published line, and no line carries a key, its hash, a client version or anything that links two verdicts to one person.

**The manifest:**
```json
{ "upnext_verdicts": 1, "date": "2026-10-09", "vocab": 1, "games": 4, "verdicts": 37,
  "licence": "CC0-1.0", "notice": "UpNext backlog verdicts: … CC0 1.0 …",
  "summary": { "file": "verdicts-summary-2026-10-09.jsonl.gz", "bytes": 1072, "sha256": "…" },
  "all": { "file": "verdicts-all-2026-10-09.jsonl.gz", "bytes": 1055, "sha256": "…" } }
```
`games` is the summary's games and `verdicts` the full file's verdicts. `bytes` and `sha256` are of each file as downloaded. The files are named for the manifest's `date`, beside it.

**A header:**
```json
{ "upnext_verdicts": 1, "kind": "summary", "date": "2026-10-09", "vocab": 1, "count": 4, "verdicts": 35,
  "sha256": "…", "licence": "CC0-1.0", "notice": "…" }
```
`kind` is `summary` or `all`. `count` is the lines after the header. `verdicts` is the verdicts the lines count: the sum of the games' `n` in a summary, `count` in the full file. `vocab` is the newest vocabulary among the verdicts.

**A game in the summary:**
```json
{ "steam_appid": 292030, "gog_id": "1207664663", "n": 6,
  "counts": { "outcome": {"finished": 3, "shelved": 1, "stopped": 2}, "worth": {"yes": 3, "for_some": 2, "no": 1},
              "hours": {"10_20": 1, "80_plus": 1}, "clicked": {"later": 2, "never": 1},
              "limited_time": {"no": 3, "mostly": 2}, "factors": {"hard_to_resume": 5},
              "stopped_for": {"too_long": 3, "pacing": 3},
              "hours_by_outcome": {"finished": {"80_plus": 1}, "stopped": {"10_20": 1}} },
  "texts": [ { "id": "…", "outcome": "stopped", "worth": "no", "hours": "10_20",
               "stop_note": "once Velen is done", "month": "2026-08" } ] }
```
- `n` is the verdicts counted, at least 3. Every count is how many verdicts gave that answer; answers nobody gave are left out, and so is a question nobody answered. `outcome` and `worth` add up to `n`; no other question's counts add up to more than `n`. `factors` and `stopped_for` count each verdict that picked the value.
- `hours_by_outcome` is the hours of the verdicts with each outcome, for the game page's "Finished: usually 10 to 20 h".
- `texts` are up to 20 verdicts with a `text`, a `stop_note` or both, newest month first and then by id, so their order says nothing finer than the month. A text held or hidden by moderation isn't there.
- **Which verdicts are one game's:** those with the same `steam_appid`; a verdict with only a `gog_id` joins the game whose verdicts carry both that GOG id and an appid (the appid most of them carry), and is a game of its own otherwise. A game's `gog_id` is the one most of its verdicts carry. `format::summarise` in `src/verdicts/format.rs` is the reference.

**A verdict in the full file** is the verdict as sent without `licence`, `consent` and `client`, with its `id` and `month`: `{"id": "…", "steam_appid": 1145360, "outcome": "finished", "worth": "yes", "hours": "10_20", …, "vocab": 1, "month": "2026-10"}`. Ids are 16 random bytes in lowercase base32 without padding (26 characters of `a` to `z` and `2` to `7`).
