# Where things go

[Contributing](../../CONTRIBUTING.md) › Where things go. Also: [checks](checks.md).

## Sources

Everything `upnext enrich` can fetch is a list in `src/enrich/sources.rs`. A new source is one file under `src/enrich/sources/` and one line in `registry()`.

- A `TitleSource` is fetched per title, in a lane of its own (one lane per rate-limited host). It names its options (`Opt::new("id", "Label")`, `.off()` for opt-in, `.blocked(...)` when it needs a key or a setting), says which titles it `needs`, and `run`s for one title. `src/enrich/sources/protondb.rs` is a short example.
- A `Pass` runs once over the whole library, before or after the lanes: the AI summaries, series guides and the wishlist.
- The CLI's `--only`, `--skip` and `--with`, the jobs API and Settings all read the list, so a new source shows up in each without further wiring.
- The client for the service goes in its own module (`src/protondb.rs`) and into `Clients` in `src/enrich.rs`. It takes a base URL so tests can point it at a `wiremock` server; tests never call the real service.
- Add the service to its page in [Third-party services](../THIRD_PARTY.md) (what is called, the terms, what they ask for), and its attribution to Settings › About (`web/src/lib/credits.ts`) if its terms want one.

## Stores

`src/steam.rs`, `src/steam_local.rs`, `src/heroic.rs`, `src/gog.rs` and `src/galaxy.rs`. A sync upserts games keyed by `(source, source_id)`, and titles group copies across stores (`src/db/titles.rs`).

## The HTTP API

`src/server.rs` and `src/server/`, versioned under `/api/v1/`. Each area's handlers are a module in `src/server/` with its own `routes()`, which `router` in `src/server.rs` merges. The state handlers are given is in `state.rs`, and `ApiError` in `errors.rs`.

A new route needs a `#[utoipa::path]` attribute and an entry in `src/server/openapi.rs`. Then `UPNEXT_UPDATE_OPENAPI=1 cargo test openapi` rewrites `docs/openapi.json`, `npm run gen:api` rewrites `web/src/generated/api.ts`, and the route's page in `docs/api/` (listed in `docs/API.md`) gets a paragraph, all in the same commit. CI fails when the document or the types are stale, or when a path is missing from the API guide.

## Settings

`src/config.rs` and `src/config/`. A setting is a field on `Config` and one row of `SETTINGS` in `src/config/settings.rs`, which gives its key, its environment variable, its default, whether the app can change it, how a value typed there is checked (`src/config/check.rs`), and whether it's secret. Loading (`src/config/load.rs`), the Settings page, `config list` and the exports all read that table.

## The schema

`src/db/schema.rs`. A new database gets `SCHEMA`; an older one backs itself up and upgrades itself on open through the steps in `UPGRADES`. Versions start at v29 (`BASELINE_VERSION`), the schema of the first release, and a database from an earlier development build is refused.

A change bumps `SCHEMA_VERSION`, changes `SCHEMA`, and adds the step that brings a database at the version before to the same tables. Then `UPNEXT_UPDATE_SCHEMA_FIXTURE=1 cargo test write_current_fixture` writes `tests/fixtures/schema/vN.sql` for the new version. `src/db/migration_tests.rs` checks that every version since the baseline upgrades to a new database's tables and keeps its rows; add a test there for any rows the step moves or repairs.

## The AI

`src/llm/`: every backend implements `LlmBackend`. Prompts are built in `src/prompt.rs`, `src/digest.rs` and their neighbours, and a pick prompt's request words, ranking and game lines are in `src/prompt/`. Replies are checked against the candidates in `src/recommend.rs`.

## The UI

`web/src/`: pages in `pages/`, their parts in `components/<page>/`, and logic worth a test in `lib/`, with a `*.test.ts` beside it.

## The Android app

`android/`, in Kotlin and Gradle. `core/` is plain Kotlin with no Android in it (addresses, the announcement, saved computers, the pairing calls), with tests that run on any JVM: `cd android && ./gradlew -Pupnext.coreOnly=true :core:test`, which needs no Android SDK. `app/` is the Android app (the screens, discovery, the keystore and the WebView), and needs JDK 17 and the Android SDK: `./gradlew :app:assembleDebug`. The UI itself is the web UI, loaded from the computer; the app adds only the screens that come before it. The `android` workflow builds it when `UPNEXT_ANDROID_CI` is on.

## The Flatpak

`contrib/flatpak/`: the manifest, its launcher entry and AppStream metadata, and the scripts that write its offline sources, build it and try it ([its README](../../contrib/flatpak/README.md)). A new crate or npm package needs nothing there, because the sources are generated from the lock files at each build. A new folder outside UpNext's own that the app reads on Linux needs a read-only `--filesystem` line in the manifest, with a reason in that README. `src/flatpak.rs` is where the app learns it runs in one. The `flatpak` workflow builds it when `UPNEXT_FLATPAK_CI` is on.

## The Steam copy

`contrib/steam/`: the scripts that lay out its two depots from a release, run the Linux one without GTK or WebKitGTK and upload them with steamcmd, the SteamPipe build files, the store text and the library images ([its README](../../contrib/steam/README.md)). A new library the app links on Linux comes into the Linux depot by itself; one that must come from the computer instead, as part of the graphics driver's stack, goes in `contrib/steam/linux-exclude.txt` with the reason. `src/steam_copy.rs` is where the app learns it's the Steam copy.

## The backlog verdicts service

`services/verdicts/`: a Cloudflare Worker that takes verdicts and publishes the daily snapshot, following [the contract](../VERDICTS.md) and reading its tables from `docs/verdicts-contract.json`. [Its README](../../services/verdicts/README.md) says how to run and test it, and `docs/dev/VERDICTS-SERVICE.md` is the operator's runbook.
