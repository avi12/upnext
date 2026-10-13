# Checks

[Contributing](../../CONTRIBUTING.md) › Checks. Also: [where things go](code-map.md).

CI (`.github/workflows/ci.yml`) runs all of these. Run them before you push.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo clippy --all-targets --locked --no-default-features -- -D warnings
cargo test --locked
cargo deny --locked check                # licences, advisories, bans, sources (deny.toml)

cd web
npm run licenses                         # every npm package's licence is on the allowed list
npm run gen:api && git diff --exit-code src/generated/api.ts
npx tsc -b
npm run lint                             # oxlint
npm test                                 # Vitest
npx vite build
```

`cargo deny` comes from `cargo install --locked cargo-deny`.

## Third-party notices

After a change to `Cargo.lock`, `web/package-lock.json`, `android/gradle/libs.versions.toml` or `web/src/lib/credits.ts`, regenerate the third-party notices every release carries, and commit them with the change. The `notices` workflow fails until they match.

```sh
cargo install --locked cargo-about --version 0.9.2 --features cli   # once
cargo fetch --locked && (cd web && npm ci)
node contrib/notices/generate.mjs           # --check only compares
```

`docs/dev/RELEASE.md`, "Third-party notices", says what goes in them.

## The backlog verdicts service

It has its own checks, in its own folder, and CI runs them too:

```sh
cd services/verdicts
npm ci
npm run licenses && npm run typecheck && npm run lint && npm test && npm run build
```

## End-to-end test

`web/e2e/` drives the real server on the mock library, with the fake model, in Chromium:

```sh
cargo build --locked --no-default-features    # or set UPNEXT_BIN to a binary
cd web
npx vite build
npx playwright install chromium               # once
npm run e2e
```

`web/e2e/serve.sh` starts the server in a throwaway folder with its own `HOME`, so nothing on your machine is read.

The README's screenshot, `docs/images/pick.png`, and the store listings' big-screen one, `docs/images/big-screen.png`, are taken the same way: `UPNEXT_README_SHOT=1 npx playwright test readme-shot` writes both after Curate on the mock library. The covers come from Steam's image servers, so it needs a machine that reaches them, and it fails rather than save a shot whose covers are name cards.

## Windows

The `windows` workflow builds and tests every Rust change on Windows. Check code that touches paths, processes or the registry there: the unit tests that run shell scripts as stand-in CLIs are `#[cfg(unix)]`.
