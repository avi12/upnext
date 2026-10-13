# AI usage

[API index](../API.md) · [Prices and plans](ai-prices.md) · [AI backends](ai-backends.md)

Every AI call is recorded when it comes back, whether it worked or not: when, what for, the backend and model, the time it took and the tokens its backend reported. Records are kept for a year.

| `kind` | What | Counted per |
|---|---|---|
| `pick` | Picks, a Curate again, and new spares | pick |
| `too` | "It's too ___" | call |
| `decide` | Help me decide | call |
| `review_summaries` | People say, Vibes and review tags | title |
| `series_guides` | Series guides | series |
| `second_chances` | Second Chance's "Now" | game |
| `hidden_gems` | Find hidden gems' ratings | game |
| `test` | Settings' "Test" button | call |

A lineup's `usage` adds up every call made with it: the pick, its retry, "It's too ___" and new spares.

## Usage and totals

`Usage`, the tokens of a call or a sum of calls:

```json
{"input_tokens": 29212, "output_tokens": 302, "cache_read_tokens": 0, "cache_write_tokens": 29210, "cache_write_1h_tokens": 29210}
```

- `input_tokens` is all of the input. `cache_read_tokens` and `cache_write_tokens` are the parts read from or written to the provider's prompt cache. Through the Claude CLI nearly all input is written to the cache, kept for an hour (`cache_write_1h_tokens`), which costs twice the input price rather than 1.25 times.
- `output_tokens` includes a reasoning model's thinking.
- OpenAI-style servers report cached input only when they cache (vLLM doesn't), and never cache writes.

`AiTotals`, calls added up:

```json
{"calls": 12, "failed": 0, "input_tokens": 196691, "output_tokens": 15389, "cache_read_tokens": 40302, "cache_write_tokens": 156361,
 "cache_write_1h_tokens": 156361, "cost_usd": null, "api_usd": 0.79, "would_cost_usd": 0.787, "items": 40, "duration_ms": 120059}
```

- `failed` is how many of `calls` failed. A failed call counts the tokens its reply used when it came back unusable (cut off, not JSON), and none otherwise.
- `cost_usd` is money charged, at list prices: an API key, or a hosted provider whose prices UpNext reads. Null when nothing was charged.
- `api_usd` is every call at the API's list prices, charged or not, from the price list in use when it was made. On a subscription it's what the same calls would cost on the API, never money spent. Null when no call had a known price, as with a local model.
- `would_cost_usd` is what the Claude CLI itself said its calls would cost on the API. It's kept but not shown, since `api_usd` uses one price list for every backend so they compare.
- `items` is the titles or series the calls that worked covered.

## GET /api/ai/usage?days=30

The last `days` days (1 to 365, 30 unless asked), today included, in this computer's local days:

```json
{"days": 30, "backend": "claude-cli", "priced": false,
 "today": AiTotals, "last_7_days": AiTotals, "total": AiTotals,
 "by_day": [{"day": "2026-09-30", "...AiTotals": 0, "by_kind": [{"kind": "pick", "...AiTotals": 0}]}],
 "by_kind": [{"kind": "pick", "...AiTotals": 0}],
 "by_model": [{"backend": "claude-cli", "model": "claude-sonnet-5-5", "...AiTotals": 0}],
 "by_player": [], "by_account": []}
```

- `backend` is the one in use. `priced` is true when a backend in use is charged per call at prices UpNext reads (the Anthropic key, the OpenAI key, or another provider at `api.openai.com` or `openrouter.ai`). Money shows only with "Show costs in US dollars" on.
- `today` and `last_7_days` are there whatever `days` is; `total` covers the `days`.
- `by_day` lists every day of the period, oldest first, days without calls included. `by_kind` is in the table's order, and `by_model` has the most tokens first.
- `by_player` and `by_account` break it out by who asked and by AI account ([Players](players.md#elsewhere)).

What each feature would take on this library is on [AI estimates](ai-estimates.md).

A job's log gets a line when an AI pass ends ("4 calls used 97.5k tokens in and 12.4k out"), and `upnext pick` prints the pick's tokens under the lineup.
