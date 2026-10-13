## What this changes

<!-- What someone using UpNext would notice, before and after. Link the issue if there is one. -->

## How it was checked

<!-- The checks you ran (docs/contributing/checks.md), and anything you tried by hand: which page, which command, on the mock library or your own. -->

- [ ] `cargo fmt --check`, `cargo clippy` (both feature sets) and `cargo test` pass
- [ ] In `web/`: `npx tsc -b`, `npm run lint`, `npm test` and the build pass
- [ ] An API change updates its page in `docs/api/`, `docs/openapi.json` and `web/src/generated/api.ts`
- [ ] A new service is in `docs/third-party/`
- [ ] No keys, tokens, databases or data from a real library in the diff
