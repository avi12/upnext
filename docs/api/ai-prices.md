# Prices and plans

[API index](../API.md) · [AI usage](ai-usage.md) · [AI estimates](ai-estimates.md)

## Prices

Each call is priced when it comes back, from the list prices of the provider its backend uses:

| Backend | Provider | Charged |
|---|---|---|
| Claude subscription (`claude-cli`) | Anthropic | no: `api_usd` is what it would cost on the API |
| ChatGPT subscription (`codex-cli`) | OpenAI | no: `api_usd` is what it would cost on the API |
| Anthropic API key (`api-key`) | Anthropic | yes |
| OpenAI API key (`openai-key`) | OpenAI | yes |
| Another provider at `api.openai.com` | OpenAI | yes |
| Another provider at `openrouter.ai` | OpenRouter | yes |
| Any other server (local models, Groq, Gemini…) | none | no figure |

UpNext reads the prices from the provider's own page: Anthropic's and OpenAI's pricing pages as markdown (`…/pricing.md`), and OpenRouter's models API. They're read again when a week old, and a failed read waits a day; until a read works, the last prices read stay in use. Anthropic's fall back to a list built into UpNext; the others have none, so their calls have no figure until a read works. OpenRouter prices only calls made through OpenRouter, since its prices for other providers' models can differ from theirs.

A model is priced by its own id, or by the longest listed id it extends (`claude-sonnet-5-5-20261001` as `claude-sonnet-5-5`). Where a page prices a model by prompt length, the price for shorter prompts is used, since UpNext's prompts are well under the threshold. Cache reads and writes have their own prices; where a page lists none, cache reads cost the input price and writes 1.25 times it (five minutes) or twice (an hour).

### GET /api/ai/prices

The prices the AI in use is priced at, read first when they're a week old:

```json
{"backend": "claude-cli", "charged": false,
 "prices": {"provider": "anthropic", "label": "Anthropic", "page_url": "https://platform.claude.com/docs/en/about-claude/pricing",
   "fetched_at": 1759280000, "built_in_checked": null, "error": null, "tried_at": 1759280000,
   "models": {"claude-sonnet-5-5": {"input": 2.0, "output": 10.0, "cache_read": 0.1, "cache_write_5m": 2.5, "cache_write_1h": 4.0}}}}
```

Prices are US dollars per million tokens, and `page_url` is the official page, to link wherever a price shows. `fetched_at` is when they were read; it's null, with `built_in_checked` the date the built-in list was checked, while that list is in use. `error` is why the last read failed, at `tried_at`. `prices` is null for a local model or a provider whose prices UpNext can't read. With a separate summaries backend, both backends' prices are read, and the answer describes the picks'.

`POST /api/ai/prices/refresh` reads them again now, in the same shape; a failure is in `prices.error` and the prices in use stay as they were.

## GET /api/ai/plan

On a subscription, the plan's usage bars and what UpNext's AI takes of them. It reads the bars now, unless a reading is under 30 seconds old.

```json
{"backend": "claude-cli", "subscription": true, "plan": "max",
 "reading": {"at": 1759280000, "windows": [
   {"id": "session", "label": "Current session", "used": 20.0, "resets": "Sep 30, 6:30pm (America/New_York)", "resets_at": null},
   {"id": "week", "label": "Current week (all models)", "used": 40.0, "resets": "Oct 4, 3pm (America/New_York)", "resets_at": null}]},
 "error": null,
 "measured": {"measure": "api_usd", "spans": 3,
   "windows": [{"id": "session", "label": "Current session", "per_point": 0.36, "spans": 3, "from_session": null},
               {"id": "week", "label": "Current week (all models)", "per_point": 1.98, "spans": 0, "from_session": 5.5}],
   "each": [{"kind": "pick", "items": 1, "percent": {"session": 0.33, "week": 0.06}}],
   "one_time": {"kind": "review_summaries", "items": 200, "percent": {"session": 41.0, "week": 7.5}}}}
```

- `subscription` is true on the Claude CLI and the [Codex CLI](ai-codex.md#the-plans-bars); for anything else it's false and the rest is null.
- `plan` is what `claude auth status` calls it ("max", "pro"). Nothing local says a Claude plan's tier, and UpNext doesn't know any plan's limits.
- `reading.windows` has the limits the plan has; one it doesn't have is absent, never 0 or 100. `used` is always the percent used: a line that counts down ("80% left") is turned round, and one whose direction can't be told is left out. `resets` is the Claude CLI's own wording.
- `error` says why the bars couldn't be read, with `reading` null. On the Claude CLI the read is `claude -p /usage`, which the CLI answers itself without spending anything. UpNext checks the reply says so, and if a CLI ever sends it to the model instead, it stops reading until the CLI's version changes.
- UpNext reads the bars around its own calls (before a call when the last reading is a minute old, and after each call) and keeps the readings for a year. Each AI account's readings are its own.
- `measured` is null until a measurement can be used. A measurement runs from one tick of a bar to the next, when only UpNext's calls fill it (no gap over 10 minutes, at most 3 hours) and no reset falls in it. `per_point` is what one point took in `measure`: `api_usd` or `tokens`, whichever varies less once there are 4 measurements. The figure is the largest recent measurement or the median of the top three, since other use of the same plan only makes UpNext's calls look dearer. A window with `from_session` was worked out from the session's figure.
- `each` is what one pick, decide, title of summaries or series guide takes of each window, in percent, and `one_time` the summaries still to write. A change of plan starts the readings over.
