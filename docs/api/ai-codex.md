# ChatGPT through the Codex CLI

[API index](../API.md) · [AI backends](ai-backends.md) · [Prices and plans](ai-prices.md)

`llm_backend` `codex-cli` runs picks, Help me decide, review summaries and series guides on a ChatGPT plan through `codex exec`. You sign in with `codex login`; UpNext only runs the program and never reads Codex's credentials.

- `codex_model` (picks, series guides, Help me decide) and `codex_digest_model` (review summaries): a model id or a family, `luna` unless set.
- `codex_effort`: the reasoning effort, one of `codex_efforts` (`low`, `medium`, `high`, `xhigh`, `max`, `ultra`; a model takes some of them), or null for Codex's default for the model. Anything else is a `400`.
- Env `UPNEXT_CODEX_MODEL`, `UPNEXT_CODEX_DIGEST_MODEL` and `UPNEXT_CODEX_EFFORT`, and `UPNEXT_CODEX_BIN` (env only, `codex` unless set).

Each call checks `codex login status` first, which takes milliseconds, so a signed-out Codex fails at once with "Codex isn't signed in: run `codex login` in a terminal, then try again". A sign-in with an API key is refused, since it bills each call to the key; use the [OpenAI API key](ai-openai-key.md) backend for that. Calls are recorded with backend `codex-cli` and the model they resolved to, priced at OpenAI's list prices and not charged: `api_usd` is set and `cost_usd` is null.

## Model families

A family is any `gpt-<version>-<name>` id in the account's catalog, and its name means the newest of them, whatever its version, the way `sonnet` means the newest Sonnet on the Claude CLI. So `luna` is the newest Luna. Each call resolves it.

## GET /api/llm/codex

The Codex CLI, for Settings, from `codex --version`, `codex login status` and `codex debug models`, all local:

```json
{"installed": true, "version": "codex-cli 0.159.2",
 "login": {"signed_in": true, "status": "Logged in using ChatGPT", "per_call": false},
 "error": null,
 "models": [{"id": "gpt-6-luna", "name": "GPT-6-Luna", "description": "Fast and affordable model for easier tasks.",
             "efforts": ["low", "medium", "high", "xhigh", "max"], "default_effort": "medium"}],
 "families": [{"id": "luna", "model": "gpt-6-luna"}],
 "models_from": "codex", "models_error": null}
```

- `login` is null and `error` says why when `codex` can't be run. `per_call` is true for a sign-in that bills each call (an API key, Amazon Bedrock).
- `models` are the ones Codex lists for the account, in its order, and `families` each family in that order with the model it means now. When the catalog can't be read, `models_from` is `"built_in"`, `models_error` says why, and `models` is a list built into UpNext.

## The plan's bars

`GET /api/ai/plan` ([Prices and plans](ai-prices.md#get-apiaiplan)) reads a ChatGPT plan through `codex app-server`'s `account/rateLimits/read`, which answers from the account's usage without calling a model, so it spends nothing.

```json
{"backend": "codex-cli", "subscription": true, "plan": "plus",
 "reading": {"at": 1790832331, "windows": [
   {"id": "session", "label": "5-hour limit", "used": 12.0, "resets": null, "resets_at": 1790847255},
   {"id": "week", "label": "Weekly limit", "used": 30.0, "resets": null, "resets_at": 1791413286}]},
 "error": null, "measured": null}
```

- A window's `id` comes from its length: 300 minutes is `session`, 10,080 is `week`, and any other is `<minutes>m` ("1440m", labelled "1-day limit"). A limit of its own for one model adds its name after a colon, `week:<model>`, labelled "Weekly limit (<model>)". A limit the plan doesn't have is absent.
- `resets_at` is in Unix seconds and `resets` is null. `plan` is the account's `planType` ("plus", "pro", "team"…).
- `error` is "`codex app-server` said: …" when the server answers with an error (signed out: "codex account authentication required to read rate limits"), and "couldn't read your plan's usage: …" when the answer has a shape UpNext doesn't know.
