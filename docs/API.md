# UpNext HTTP API (v1)

The UpNext server answers on `http://127.0.0.1:8787` unless set otherwise. The UI uses this API, and so can scripts and other apps.

- **Versions.** The contract is `/api/v1/…`. Adding a field or an endpoint stays within v1; removing or renaming one would mean v2. Unversioned `/api/…` paths answer the same as an alias outside the contract; these pages write paths that way for short, but scripts should use `/api/v1/…`.
- **The precise contract** is the OpenAPI document at `GET /api/v1/openapi.json`, made from the code and kept as [`docs/openapi.json`](openapi.json). These pages are the readable guide.
- **Bodies** are JSON. Times are Unix seconds and playtime is minutes unless a field says otherwise. Errors are `{"error": "…"}` with a plain reason, even for a path, query or body the server can't read. A `500` gives the real reason only on the computer UpNext runs on; any other device reads "Something went wrong on the server. UpNext's log on the computer it runs on says what."
- **Who it acts for.** Most routes answer for one player: the token's, the `UpNext-Player` header's, the browser's choice, or the default ([Players](api/players.md)).

## Access

- [Who can use the API](api/access.md): this computer, tokens, scopes, hosts and limits
- [Only on this computer](api/local-only.md)
- [Tokens and pairing](api/pairing.md) · [Allowing a device with a tap](api/allowing.md)
- [Other devices and computers on the network](api/network.md)
- [public_url and proxies](api/proxies.md) · [Signing in with an identity provider](api/oidc.md)

## The library

- [The server, its profile and its numbers](api/basics.md): health, config, stats, logs
- [Games: the GameView](api/games.md) · [The library](api/library.md): search, Play later, Help me decide
- [Titles and copies](api/titles.md) · [Statuses, staples and stop reasons](api/statuses.md)
- [Game details](api/details.md) · [Review tags](api/review-tags.md) · [Series](api/collections.md)
- [Shared game data](api/seed.md)

## Picks

- [Picks](api/picks.md): asking, and history
- [Lineups](api/lineups.md): spares, the card's actions, "It's too ___"
- [Picks without AI](api/picks-without-ai.md) · [Ratings](api/ratings.md) · [Routines](api/routines.md)
- [Discovery: new to you, and roll the dice](api/discovery.md) · [Hidden gems](api/gems.md)
- [Second chances](api/second-chances.md)
- [Wishlist](api/wishlist.md) · [Wishlist in picks](api/wishlist-picks.md)

## Taste

- [Taste profile](api/taste.md) · [Patterns, and how picks read the profile](api/taste-patterns.md)
- [What UpNext knows about you](api/taste-sources.md) · [First-run questions and the play diary](api/diary.md)

## Backlog verdicts

- [Your own verdicts](api/verdicts.md) · [Sending verdicts](api/verdicts-sending.md)
- [Backlog verdicts from others](api/community-verdicts.md)

## Stores and sync

- [Steam](api/steam.md) · [GOG](api/gog.md) · [Epic, Galaxy and the other stores](api/more-stores.md)
- [Big screen and the Steam shortcut](api/steam-shortcut.md)
- [Jobs](api/jobs.md) · [Starting and following jobs](api/job-routes.md)
- [Scheduled jobs](api/schedules.md) · [Sync and enrich sources](api/sources.md)
- [Backups](api/backups.md) · [Restoring and importing](api/restore.md)

## AI

- [AI backends](api/ai-backends.md): Claude models, a separate AI for summaries, the test call
- [ChatGPT through the Codex CLI](api/ai-codex.md) · [OpenAI API key](api/ai-openai-key.md) · [Another provider](api/ai-openai-compatible.md)
- [AI usage](api/ai-usage.md) · [AI estimates](api/ai-estimates.md) · [Prices and plans](api/ai-prices.md)

## Settings, setup and players

- [Settings](api/settings.md)
- [First-run setup](api/setup.md) · [What the wizard reads](api/setup-wizard.md) · [Background work and uploads](api/setup-background.md)
- [Players](api/players.md) · [Hiding games](api/hiding.md) · [Store accounts](api/accounts.md) · [AI accounts](api/ai-accounts.md)
