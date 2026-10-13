# AI estimates

[API index](../API.md) · [AI usage](ai-usage.md) · [Prices and plans](ai-prices.md)

## GET /api/ai/estimates

What each AI feature would take on this library, for the wizard's AI step and a job's estimate:

```json
{"backend": "claude-cli", "priced": false, "features": [
  {"kind": "review_summaries", "backend": "claude-cli", "model": "sonnet",
   "rate": {"input_tokens": 4060, "output_tokens": 520, "items_per_call": 6.0, "seconds_per_call": 21.4, "api_usd": null, "basis": "built_in", "calls": 0},
   "one_time": {"items": 1600, "calls": 267, "input_tokens": 6496000, "output_tokens": 832000, "cost_usd": null, "minutes": 31.8, "basis": "library"},
   "monthly": {"items": 16, "calls": 3, "input_tokens": 64960, "output_tokens": 8320, "cost_usd": null, "minutes": 0.4, "basis": "assumed"}}]}
```

- `rate` is per title (`review_summaries`), per series (`series_guides`) or per call (`pick`, `decide`, `too`). Its `basis` is `history` once this install has 3 calls of that kind that worked through the backend in use in the last 90 days (`calls` says how many), else `built_in`. Other backends' calls don't count, as the CLIs add tokens of their own and models count tokens differently. `seconds_per_call` is null for an OpenAI-style server until it has been used.
- `one_time` is the run for what doesn't have it yet: titles with reviews and no summary (or one from an older prompt), and series with no guide (or whose owned games changed). Null for the features that run only when asked.
- `monthly` is a typical month: picks and decides at the rate of the last 30 days, and new games at the rate syncs brought them over the last 90 (leaving out the first sync), times the share with reviews or in a series. Until the library is 14 days old these are assumed: 20 picks, 4 decides and 8 "It's too ___" a month, and 1% of the library in new games (at least 5).
- `minutes` counts the batch passes' calls three at a time. `cost_usd` is set only for a backend charged per call, at list prices, with no prompt caching assumed. `model` is the setting it uses, or with the API key the model id it resolves to.
