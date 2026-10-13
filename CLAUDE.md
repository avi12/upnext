# UpNext

A local, single-user game-backlog picker: a Rust server (axum, rusqlite, clap) in `src/` and a React, Vite and Tailwind UI in `web/`. README.md says what it does; CONTRIBUTING.md has setup, with the checks and where things go in `docs/contributing/`. This file is the short form for Claude Code.

## Checks before a commit

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo clippy --all-targets --locked --no-default-features -- -D warnings
cargo test --locked
cargo deny --locked check
node contrib/notices/generate.mjs --check
(cd web && npm run licenses && npm run gen:api && git diff --exit-code src/generated/api.ts && npx tsc -b && npm run lint && npm test && npx vite build)
(cd services/verdicts && npm ci && npm run licenses && npm run typecheck && npm run lint && npm test && npm run build)
```

This is what CI runs (`.github/workflows/ci.yml`); `docs/contributing/checks.md` is the long form. Keep the three in step.

- The default build links WebKitGTK and GTK 3 (`libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config`). `--no-default-features` builds the server alone.
- End-to-end: `cargo build --no-default-features`, then in `web/`, `npx vite build && npm run e2e`.

## Rules

- Never use a real library or real keys. `upnext mock-library` fills an empty database (set `UPNEXT_DATA_DIR` to a scratch folder first). For AI calls, use `upnext prompt` (prints, sends nothing) or the fake model in `web/e2e/fake-llm.mjs`; when real calls are needed, use the API key's defaults: Claude Sonnet 5.5 (`claude-sonnet-5-5`) for picks and Claude Haiku 5.5 (`claude-haiku-5-5`) for summaries.
- Tests never call real services: clients take a base URL, and tests point it at `wiremock`.
- Never commit a database, `.env`, tokens or data cut from a real library. The repo is public.
- Refactors change structure only; a behaviour change goes in its own commit with a test.
- API change: `#[utoipa::path]` and `src/server/openapi.rs`, then `UPNEXT_UPDATE_OPENAPI=1 cargo test openapi`, `cd web && npm run gen:api`, and the route's page under `docs/api/` (indexed in `docs/API.md`) in the same commit.
- Schema change: bump `SCHEMA_VERSION` in `src/db/schema.rs` with a migration (change `SCHEMA` and add a step to `UPGRADES`) and a test in `src/db/migration_tests.rs`, and write the new version's fixture with `UPNEXT_UPDATE_SCHEMA_FIXTURE=1 cargo test write_current_fixture`. Databases older than v29 (`BASELINE_VERSION`) are refused, never upgraded.
- A dependency change (`Cargo.lock`, `web/package-lock.json`, `android/gradle/libs.versions.toml`) or a new credit: regenerate the third-party notices with `node contrib/notices/generate.mjs` (cargo-about 0.9.2, `cargo fetch --locked`, `web/node_modules`) and commit `THIRD_PARTY_NOTICES.txt` and the Android app's copy with it.
- A new data source: one file under `src/enrich/sources/` plus one line in `registry()` (`src/enrich/sources.rs`), an entry in `docs/third-party/` (indexed in `docs/THIRD_PARTY.md`), and its attribution in `web/src/lib/credits.ts` if its terms ask for one.
- HowLongToBeat is scraped and stays off unless the user turns it on (its job's schedule, or `--with hltb`).
- `unsafe` is forbidden (`Cargo.toml`, `[lints.rust]`).
- Comments and docs are plain, full sentences that say why, matching the code around them.

## Runbooks

`docs/dev/` holds the maintainers' runbooks: `RELEASE.md` (releases, and the steps that only happen once), `CI-RUNNERS.md` (self-hosted runners while the repository is private) and `VERDICTS-SERVICE.md` (running the backlog verdicts service); `docs/dev/README.md` lists them.
