# OpenAI API key

[API index](../API.md) · [AI backends](ai-backends.md) · [Prices and plans](ai-prices.md)

`llm_backend` `openai-key` runs picks, Help me decide, review summaries and series guides on the OpenAI API with your own key, billed per call.

- `openai_key`: the key, write-only. It must start with `sk-`; an Anthropic key (`sk-ant-`) is a `400`. `GET` gives `openai_key_set` and `openai_key_source`. It's masked in `config list` and left out of exports and copies without keys. Env `OPENAI_API_KEY`.
- `openai_key_model` (picks, series guides, Help me decide) and `openai_key_digest_model` (review summaries): a model id or a family, `luna` unless set. Env `UPNEXT_OPENAI_KEY_MODEL` and `UPNEXT_OPENAI_KEY_DIGEST_MODEL`.
- `openai_key_effort`: one of `openai_key_efforts` (`low`, `medium`, `high`, `xhigh`, `max`), or null for OpenAI's default for the model. Anything else is a `400`. Env `UPNEXT_OPENAI_KEY_EFFORT`.
- `UPNEXT_OPENAI_URL` (env only, `https://api.openai.com/v1` unless set), for a mock.

## Each call

`POST {url}/chat/completions` with the key as a bearer token:

- `response_format` is `json_schema` with `strict: true`, and the schema adapted for strict mode: no size or range limits, every object closed, every property required and the optional ones nullable. The nulls that adds are taken out of the reply. There's no fallback to plain JSON mode.
- The instructions go as a `developer` message, with `store: false`, and `reasoning_effort` when one is set.
- `prompt_cache_key` is `upnext-` and 16 hex digits hashed from the instructions and the cached part (a pick's candidate list), so a Curate again shares it.
- Review summaries and series guides send `service_tier: "flex"`: OpenAI's Flex tier, slower, at the Batch prices. While Flex answers `429` "Resource Unavailable", which isn't charged, the call waits and tries again, three tries in all, then goes at the standard price. A project or model that refuses Flex (a `400` on `service_tier`) goes at the standard price at once. Picks, Help me decide and Test never use Flex.
- A family resolves to its newest model on the key's list (`GET {url}/models`, read once per backend), or on the built-in list when that can't be read. Calls are recorded with the model they resolved to; a call that failed keeps the setting's value.

Errors: a rejected key (`401`), a refusal (`403`), a model the key can't use (`404`, or a `400` on `model`), an effort the model doesn't take (a `400` on `reasoning_effort`) and a reply format OpenAI refuses (a `400` on `response_format`) fail every call the same way, so a batch stops at once. Spent credit or a spend limit (`429` `insufficient_quota` and its codes) stops it too: "the OpenAI key's credit has run out" or "the OpenAI key reached its spend limit". A plain rate limit (`429`) or a server error (`5xx`) waits for `retry-after`, else 2 seconds doubling, and tries again, three tries in all.

## GET /api/llm/openai

The GPT models the key can use, for Settings:

```json
{"key_set": true,
 "families": [{"id": "luna", "model": "gpt-6-luna"}],
 "models": ["gpt-6-luna", "gpt-5.6-luna"],
 "models_from": "openai", "models_error": null}
```

`models` are the key's models named `gpt-<version>` or `gpt-<version>-<family>`, newest first; dated snapshots and the audio, image and embedding models are left out. `families` are as for [Codex](ai-codex.md#model-families). Without a key, or when OpenAI can't be reached or rejects the key, `models_from` is `"built_in"` and `models_error` says why ("no OpenAI API key: …", "OpenAI rejected the API key (401): …").

## Prices

The key's calls are charged at OpenAI's list prices, so `cost_usd` and `api_usd` are both set. A call that ran at Flex (the reply's `service_tier` is `"flex"`) is priced from the page's Flex table, or at the standard price for a model that table doesn't list; `GET /api/ai/prices` gives `prices.flex` by model when the page has Flex prices. Estimates price summaries and series guides at Flex, and a family as its newest model on the price list. Until the key has 3 calls of a kind, its built-in rates are Codex's less the 3,226 tokens Codex adds to each call, with Codex's times.
