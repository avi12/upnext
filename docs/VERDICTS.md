# Backlog verdicts: the contract

Backlog verdicts let UpNext's players answer, for everyone, how a game fits a backlog and limited time: where they left it, whether it's worth it on a backlog, how long until it clicked, the session it needs, whether it respects limited time, when it's fine to stop and what stopped them, with an optional short text. Verdicts are sent by choice, one at a time, to a small service, and published once a day as a CC0 snapshot that installs download whole.

These pages are the contract between UpNext and that service. The service in `services/verdicts/` follows it, and anyone can run their own and point UpNext at it with the `verdicts_url` setting. The machine-readable half is [`verdicts-contract.json`](verdicts-contract.json), which `upnext verdicts contract` prints and a test keeps current. Where the pages and the code disagree, `src/verdicts/` is right.

- [Consent and keys](verdicts/consent.md): the licence, the consent screen, and the key a player sends with
- [A verdict](verdicts/verdict.md): its fields, what UpNext fills in, and what's never in one
- [Text checks](verdicts/text-checks.md)
- [The published snapshot](verdicts/snapshot.md) and [reading it](verdicts/reading.md)
- [Changing the contract](verdicts/changing.md)

The UpNext side is in the API pages ([your verdicts](api/verdicts.md), [verdicts from others](api/community-verdicts.md)), and the service's operator deploys it with [its README](../services/verdicts/README.md). `verdicts_url` is the project's own service, `https://verdicts.upnext.games`, unless set; another service is set in Settings › Library data on the computer UpNext runs on, with `upnext config set verdicts_url <address>`, or with `UPNEXT_VERDICTS_URL`.

## The endpoint

Every path is relative to `verdicts_url`. Bodies are JSON, at most 8 KB. Errors are a non-2xx status with `{"error": "<message>"}`, and a refused verdict adds `"problems": [{"field": "text", "why": "has an email address; take it out to send"}]`.

| Status | When |
|---|---|
| `400` | The body isn't a verdict: an unknown field, a missing or unknown answer, a text check, a licence other than `CC0-1.0`, a consent version older than the oldest the service accepts |
| `401` | No key, or not a key's shape |
| `403` | The key is blocked; its writes are refused and its verdicts leave the next snapshot |
| `404` | No such verdict of this key |
| `413`, `415` | A body over 8 KB, or not `application/json` |
| `429` | A limit is reached; `Retry-After` says in seconds when to try again, and UpNext keeps the verdict waiting and sends it again then, while it runs |

- **`POST /v1/verdicts`** sends a verdict, or replaces the key's earlier verdict about the same game (the same `steam_appid`, or, when either has none, the same `gog_id`). `201` for a new one and `200` for a replacement, with `{"id": "<26 characters>", "state": "live", "replaced": false}`. `state` is `held` when the game is being brigaded (below).
- **`DELETE /v1/verdicts/{id}`** withdraws one of the key's verdicts: `204`, or `404`.
- **`DELETE /v1/verdicts`** withdraws all of the key's verdicts: `200` with `{"withdrawn": 12}`. UpNext's "Take down all my verdicts and forget my key" does this, then deletes the key on the install.
- **`GET /v1/verdicts/mine`** lists the key's verdicts, so a restored install can show what it sent: `{"verdicts": [{"id": "…", "state": "live", "created_day": "2026-10-09", "updated_day": "2026-10-09", …the verdict's fields but client}]}`. `state` is `live`, `held`, `withdrawn` (until its answers are deleted) or `removed` (by a moderator).
- **`POST /v1/reports`** reports a published text: `{"verdict_id": "…", "reason": "spam"}`, where `reason` is `spam`, `personal` (personal information), `abuse` (abuse or hate) or `off_topic` (not about this game). `202` with `{}`; the same key reporting the same verdict again counts once. `404` for an id that isn't published.
- **`GET /terms`** is the service's terms and privacy page. **`/admin`** is the moderators' pages, behind Cloudflare Access, and isn't part of this contract.

### Limits

| | Limit |
|---|---|
| New verdicts per key | 20 a day |
| Replacements per key | 10 a day |
| Reports per key | 20 a day |
| Writes per client address | 60 a day across all keys. An IPv6 address counts by its /64, since a device picks new addresses in its /64 as it likes. The address is kept only as an HMAC with a salt that changes daily, in counters that expire after a day, never with a verdict |

### What the service does with them

- **Texts.** A text with reports from three different keys, each with a published verdict of its own, is held out of the next snapshot until a moderator looks; its verdict's answers stay counted unless a moderator removes the whole verdict. A key costs nothing to make, so reports from keys with no published verdict don't hold a text, though the moderators see them.
- **Brigading.** A game that gets more than 20 new verdicts in a day, or ten times its usual rate, has its new verdicts held: counted only after a moderator releases them, or after seven days with no reports.
- **Withdrawn verdicts** are left out of the next snapshot. That daily run also deletes their answers and text, keeping only the id, the game, the key's hash and the days, so an id is never used twice and a key that withdraws and sends again still counts against its limits.
