# AI backends

[API index](../API.md) · [AI usage](ai-usage.md) · [Settings](settings.md)

`llm_backend` chooses where picks, Help me decide, review summaries and series guides go. `GET /api/settings` lists the choices as `llm_backends`:

| `llm_backend` | What | Page |
|---|---|---|
| `claude-cli` (the default) | a Claude subscription, through the Claude Code CLI | below |
| `codex-cli` | a ChatGPT subscription, through the Codex CLI | [Codex CLI](ai-codex.md) |
| `api-key` | an Anthropic API key, billed per call | below |
| `openai-key` | an OpenAI API key, billed per call | [OpenAI API key](ai-openai-key.md) |
| `openai` | any OpenAI-compatible server: a model at home or another provider | [Another provider](ai-openai-compatible.md) |

The backend is built from the settings on every request, so a change applies without a restart. Clearing a setting (`null` or `""`) falls back to `.env`. `GET /api/config` gives `llm_backend`, `claude_available` (whether it can run) and `llm_error` (why not, such as "Codex isn't signed in: …"). Each player can have AI accounts of their own ([AI accounts](ai-accounts.md)).

## Claude models

`model` is for picks, series guides and Help me decide, and `digest_model` for review summaries and review tags. On the Claude CLI both are `sonnet` unless set; `opus` and `default` (the CLI's own default) are the other choices, as `models` and `digest_models` list them.

With the API key (`anthropic_api_key`, write-only, starting `sk-ant-`; `GET` gives `anthropic_api_key_set` and `anthropic_api_key_source`), `sonnet`, `opus` and `haiku` mean `claude-sonnet-5-5`, `claude-opus-5-5` and `claude-haiku-5-5`, and each feature has its own default: picks, series guides and Help me decide use Sonnet 5.5, and review summaries and tags use Haiku 5.5. Those apply while the setting was never chosen or is `default`, and `GET /api/settings` then shows `default`; `models` offers `haiku` too. Review summaries go to Haiku at `output_config.effort: "low"`, since at its default effort a batch of 8 could spend all 16,000 output tokens thinking. Haiku takes no non-default temperature, thinking budget, prefill or `fallbacks`, so none is sent to it; Sonnet and Opus get refusal fallbacks (`fallbacks: "default"`).

## A separate AI for summaries

Review summaries and tags, Second Chance's "Now" and hidden gem ratings can go to another backend, such as a Claude subscription for picks and a model at home for this bulk work. Picks, series guides, Help me decide and "It's too ___" stay on `llm_backend`.

- `digest_backend` (`UPNEXT_DIGEST_LLM`): `same` (the default) or one of `llm_backends`. `GET` lists `digest_backends` (`same` first) and `digest_models`, the Claude models the summaries' backend offers (`haiku` with the API key).
- A backend chosen here uses its own settings, as it does for picks (its address, key, thinking and effort), with its summaries model: `digest_model`, `codex_digest_model`, `openai_key_digest_model`, or `openai_digest_model` falling back to `openai_model`. So two different OpenAI-compatible servers aren't possible; one of them can be the OpenAI API key backend instead.
- Elsewhere: `ai` in `GET /api/gems` and `GET /api/second-chances` is the summaries' backend's state. `GET /api/ai/estimates` gives each feature its `backend`. `GET /api/ai/plan` reads the picks' subscription, or the summaries' one when only summaries run on a subscription. Prices are read for both backends. `upnext doctor` adds a "Summaries backend" line, and `upnext seed generate` writes on the summaries' backend.

## POST /api/llm/test

A tiny JSON round trip through the backend: `{"ok": true, "backend", "model", "duration_ms", "usage", "cost_usd", "api_usd"}` or `{"ok": false, "backend", "error"}`, always `200`. This is Settings' "Test" button, and the call is recorded like any other (kind `test`). `?purpose=summaries` tries the summaries' backend and model; without it, the picks'. `backend` says which answered.
