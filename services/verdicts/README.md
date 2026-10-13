# UpNext backlog verdicts service

The small service behind backlog verdicts: players' answers about how a game fits a backlog and limited time, sent by choice from UpNext and published once a day as a CC0 snapshot that every install downloads whole. It's a Cloudflare Worker with a D1 database for the verdicts and an R2 bucket for the snapshot, sized for Cloudflare's free tier.

It follows [the contract](../../docs/VERDICTS.md). The questions, answers, text checks and consent text come from `docs/verdicts-contract.json`, which UpNext writes (`upnext verdicts contract`); the Worker imports that file, so it checks verdicts with the app's own tables and patterns rather than a copy.

## What it does

| Path | What |
|---|---|
| `POST /v1/verdicts` | Send a verdict, or replace the key's earlier one about the same game |
| `DELETE /v1/verdicts/{id}` | Withdraw one of the key's verdicts |
| `DELETE /v1/verdicts` | Withdraw all of the key's verdicts |
| `GET /v1/verdicts/mine` | The key's own verdicts, so a restored install can show what it sent |
| `POST /v1/reports` | Report a published text |
| `GET /verdicts.json` and the dated `.jsonl.gz` files | The snapshot, served from R2 with an `ETag` |
| `GET /terms` | The terms and privacy page the consent screen links to |
| `/admin` | The moderators' pages, behind Cloudflare Access |

Requests carry the player's key as `Authorization: Bearer upnext_v_…`, and the service stores only its SHA-256. Every limit, status and body is in the contract. A run at 03:17 UTC each day releases held verdicts nobody reported after seven days, deletes withdrawn and removed verdicts' answers and text, writes the day's snapshot with its manifest last, deletes dated files older than six days, and drops the rate limiter's counters and the day's address salt.

[How the service decides](../../docs/verdicts/service.md) covers what the contract leaves open: how addresses are counted, when a game's verdicts are held, and how limits and reports behave.

## Layout

- `src/index.ts`: the routes and the scheduled handler.
- `src/contract.ts`: what's read from the contract file, plus the few constants that aren't in it, each held to the app by a test.
- `src/verdict.ts`: a verdict read strictly and checked as `Verdict::check` in `src/verdicts/verdict.rs` checks it, with the same problems in the same words.
- `src/api.ts`, `src/limits.ts`: the endpoint and its limits.
- `src/snapshot.ts`, `src/daily.ts`: the snapshot's files (a port of `format::summarise` and the writer in `src/verdicts/format.rs`) and the daily run.
- `src/access.ts`, `src/admin.ts`: the Access token check and the moderators' pages.
- `src/terms.ts`, `src/html.ts`: the terms page, which quotes the consent text from the contract.
- `migrations/`: D1's schema, applied with `wrangler d1 migrations apply`.
- `test/`: Vitest, running the Worker in Miniflare.

## Running it locally

You need Node 22 or newer, and no Cloudflare account.

```sh
cd services/verdicts
npm ci
npm run migrate:local   # the local D1 database wrangler dev uses (in .wrangler/)
npm run dev             # http://localhost:8787, with the scheduled handler callable
curl "http://localhost:8787/cdn-cgi/local/scheduled?cron=17+3+*+*+*"   # run the daily snapshot
```

Port 8787 is UpNext's own default too, so with UpNext running give the service another (`npm run dev -- --port 8789`) and point UpNext at it with `UPNEXT_VERDICTS_URL=http://localhost:8789` or the `verdicts_url` setting.

## Checks

```sh
npm run licenses    # every npm package's licence is on the allowed list
npm run typecheck   # tsc on the Worker (Workers types) and on the tests (Node types)
npm run lint        # oxlint, warnings fail
npm test            # Vitest: the Worker in Miniflare, no network
npm run build       # bundles it as `wrangler deploy` would, without deploying
```

CI runs all five. Besides the service itself, the tests check it against the app: its summary of the Rust fixture's verdicts (`tests/fixtures/verdicts/`) matches the fixture's byte for byte before gzip, and the snapshot it writes for them is committed in `tests/fixtures/verdicts/service/`, which `cargo test` reads with UpNext's strict reader (`tests/verdicts_service.rs`). When the format or the output changes, rewrite it with `UPNEXT_UPDATE_VERDICTS_SERVICE=1 npm test`.

## Dependencies

Every npm package here is a development tool, pinned to an exact version: wrangler, Miniflare and workerd, esbuild, Vitest, TypeScript, oxlint, and the Workers and Node type definitions. None is bundled into the deployed Worker, which is only this folder's code and the contract file. `npm run licenses` allows the same licences as the UI's build tools, plus one exception: Miniflare depends on sharp, whose prebuilt libvips binaries are LGPL-3.0-or-later. They run only on a developer's machine or in CI and are never distributed.

## Deploying

Deploying needs the operator's Cloudflare account, and `docs/dev/VERDICTS-SERVICE.md` has every step, from making the account to moderating. In short: `npx wrangler login`, then make the D1 database and the R2 bucket. R2 must be turned on once per account in the Cloudflare dashboard (Storage & Databases › R2 Object Storage, which asks for a payment method even on the free plan) before wrangler can create a bucket; otherwise it fails with error 10042. Copy `wrangler.toml` to `wrangler.local.toml` (ignored by git), put the database's id and the operator's settings there, then run `npm run migrate:remote` and `npm run deploy`. The committed `wrangler.toml` holds placeholders only: no account id, database id, token or zone is ever committed.
