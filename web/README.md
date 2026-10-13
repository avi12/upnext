# UpNext web UI

React 19 + Vite + Tailwind v4 frontend for UpNext, the backlog picker (Steam, Heroic, GOG, Galaxy). Talks to the Rust backend via `/api` (see `../docs/API.md`).

```sh
npm ci
npm run dev     # dev server on :5173, proxies /api to http://127.0.0.1:8787
npm run build   # emits dist/, served by the `upnext` binary
npm run lint    # oxlint
npm test        # Vitest: every src/**/*.test.ts(x), in jsdom
npm run e2e     # Playwright: needs `npx vite build` and `cargo build --no-default-features` first (e2e/serve.sh)
```

The end-to-end run starts UpNext on the mock library with a fake AI and a fake verdicts service, on ports 8797 to 8799 (`UPNEXT_PORT`, `FAKE_LLM_PORT` and `UPNEXT_ONE_PLAYER_PORT` move them), so those three must be free even to run one spec. `docs/contributing/checks.md` has the whole list of checks.

The API types come from `src/generated/api.ts` (`npm run gen:api`, from `../docs/openapi.json`); `src/types.ts` aliases them where they say exactly what the UI needs and keeps the rest by hand, so a type written there is not checked against the server.

Where things go: `src/pages/` has a page for each address and `src/pages/big/` big screen's versions of them; `src/components/` the parts, by area; `src/lib/` what isn't a component. Text helpers (`plural`, `upperFirst`, durations) are in `lib/format.ts`, money in `lib/money.ts`, and the queries several pages share (settings, config, jobs and the like) have their keys and hooks in `lib/queries.ts`.
