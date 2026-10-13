# Test fixtures

Responses the parsers are tested against. Tests load them through `src/testutil.rs`. The AI backends' replies and the price pages are described in [their own README](llm/README.md).

Nothing here comes from a personal library or contains a key or an account id, and the repository is public, so keep it that way. Review texts, SteamIDs, persona names and playtimes are made up.

When trimming a live capture, replace only the text inside string values, and never change a field's type or nesting. A GOG review's `content` is an object `{title, description}`; flattening it into a string once let a broken parser pass its test while every live response failed. After editing, compare the fixture's field types with the capture.

## The schema

`schema/vN.sql` is the schema a new database at version N gets, with no rows, for each version from v29, the first release's. `UPNEXT_UPDATE_SCHEMA_FIXTURE=1 cargo test write_current_fixture` writes the current version's file when there's none, and then it never changes, because the next upgrade's test starts from it. The comments inside the stored `CREATE` statements match `SCHEMA`'s current wording; the tests leave comments out. `schema/v29-from-v4.sql` is a v4 database's schema after the development builds' upgrades took it to v29: a library kept since then has its columns in another order and an index new databases don't have.

## Captured from the live service, then trimmed to the fields the parser reads

- `steam/appdetails_620.json`: store appdetails for Portal 2. The description was replaced with a short line containing HTML entities, and one category was duplicated to exercise de-duplication.
- `steam/appreviews_620.json`: the summary counts are real. The four reviews are invented, one per filter case (English, non-English, too short, negative). The first has a `timestamp_created`, as live reviews do.
- `steam/storesearch_portal2.json`: store search for "Portal 2".
- `steam/getitems_620.json`: `IStoreBrowseService/GetItems` asset list. Screenshots were reordered to exercise the ordinal sort, and an app with no assets was added.
- `steamspy/appdetails_620.json`: the top five tags.
- `protondb/summary_620.json`: unchanged.
- `gog/v2_game.json` and `gog/reviews.json`: GOG's public store and review APIs. The structure is real, and the text is invented. The first review has the live `date` field.
- `pcgw/opensearch_portal2.json`: unchanged.
- `pcgw/parse_page.json`: the real taxonomy rows of the Portal 2 page, without the rest of the article.
- `wikidata/by_steam_appids.json`: the real row for appid 620, plus a duplicate row with a second IGDB slug and a row with a malformed appid.

The captures are here only to test the parsers, and stay under their sources' terms ([Third-party services](../../docs/THIRD_PARTY.md)), not this repository's licences. `pcgw/parse_page.json` holds PCGamingWiki contributors' rows from the page [Portal 2](https://www.pcgamingwiki.com/wiki/Portal_2), under [CC BY-NC-SA 3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/), and the Wikidata row is CC0.

## Written by npm's own shim writer

`cmd-shim` 7.0.0, as bundled with npm 10.9, with CRLF line ends kept (`.gitattributes`):

- `process/claude_npm_native.cmd`: the shim `npm install -g @anthropic-ai/claude-code` writes on Windows for Claude Code 2.1.288, whose `bin` is the native `bin/claude.exe`.
- `process/claude_npm_node.cmd`: the shim for a package whose `bin` is a node script (`cli.js`, as Claude Code's was before the native package, and as `@openai/codex`'s is).

## Checked against the live API, with values invented

The field names and types are the live API's; the responses are personal, so the values are made up.

- `gog/catalog.json`: the store's catalogue search (`catalog.gog.com/v1/catalog`). The Witcher's and The Witcher 2's ids and slugs are real; the soundtrack listed first is made up, to test that only the exact slug counts. One `id` is a number where the live API sends a string, since the parser takes both.
- `gog/filtered_products.json`: `account/getFilteredProducts`. `releaseDate` is an object `{date, timezone_type, timezone}` in the live API.

## Built from the fields the parser reads

These services need credentials or were rate-limiting, so there's no live capture behind them.

- `galaxy/galaxy-2.0.sql`: GOG Galaxy 2.0's local database, as SQL. The tables and columns follow public descriptions of the schema (community export scripts and schema dumps), with a few tables the reader doesn't use, and the rows are invented. `sqlite3 galaxy-2.0.db .schema` on a Windows computer with GOG Galaxy gives the real `CREATE` statements to compare.
- `steam/owned_games*.json`, `steam/player_summary.json`: Steam Web API (needs a key).
- `steamspy/all_page0.json`: one page of SteamSpy's catalogue (`request=all`), with invented apps besides Counter-Strike and Portal 2. One entry has review counts as strings and one has no name.
- `steam/loginusers.vdf`: the Steam client's text VDF.
- `gog/sessions.json`: `gameplay.gog.com` sessions (needs a fresh token).
- `igdb/*.json`: IGDB v4 (needs Twitch credentials).
- `rawg/search.json`: RAWG (needs a key).
- `hltb/search.json`: HowLongToBeat search (needs a token scraped from the site).
- `wikidata/by_names.json` and `wikidata/series_*.json`: in the binding format of the real `by_steam_appids` response.

## Backlog verdicts

Made up for the tests; no verdict here was sent by anyone.

- `verdicts/verdicts.json`, `verdicts/verdicts-summary-2026-10-09.jsonl.gz` and `verdicts/verdicts-all-2026-10-09.jsonl.gz`: a snapshot of invented verdicts, most about games of the mock library, written by UpNext's own writer (`fixture_verdicts` in `src/verdicts/tests.rs`) with `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts`, which a test keeps current.
- `verdicts/mock/`: the mock library's snapshot (`src/mock/verdicts.rs`), written the same way. `upnext mock-library` applies it, and the end-to-end run's fake service (`web/e2e/fake-verdicts.mjs`) serves it.
- `verdicts/text-checks.json`: texts and the check each should fail, or `null`, read by UpNext's tests and the service's so both refuse the same texts.
- `verdicts/service/`: the snapshot the backlog verdicts service writes for the same verdicts from its daily run in Miniflare, rewritten with `UPNEXT_UPDATE_VERDICTS_SERVICE=1 npm test` there. Its tests check it's current, and `tests/verdicts_service.rs` reads it with UpNext's reader.
