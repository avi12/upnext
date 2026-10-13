# AI and price fixtures

[Test fixtures](../README.md) › The AI backends' replies, and the price pages in `../prices/`.

## The Claude Code CLI

- `claude_cli_success.json`: what `claude -p --output-format json` printed for a successful call with the flags `claude_cli.rs` uses, the Settings "Test" prompt and `--model sonnet`. The session id and uuid are placeholders. Most of the input is under `usage.cache_creation_input_tokens`, not `usage.input_tokens`, and `total_cost_usd` is an API-equivalent price, not a charge.
- `claude_cli_bad_model.json`: what Claude Code 2.1.285 printed on stdout for `--model claude-nonexistent-9`, which exited with status 1. Only the session id and uuid were replaced.
- `claude_cli_usage_limit.json`: made by hand from the one above for a spent subscription limit, with `api_error_status` 429 and a `result` in the CLI's wording, because no real one was kept.
- `claude_cli_usage_command.json`: what `claude -p "/usage" --output-format json` printed, with `--tools "" --strict-mcp-config --no-session-persistence` and hooks off. The CLI answered it locally: `local_command` is "usage", `num_turns` 0, with no tokens and no cost. The three limit lines in `result` keep the CLI's wording, but their figures and time zone are made up, the part below them was shortened and made up, and the ids are placeholders. `parse_claude` in `src/ai_plan.rs` reads it.
- `claude_cli_usage_left.json`: made by hand from the one above, for a CLI that counts down. The session line says "80% left" and the week "60% remaining", which are 20% and 40% used, and the third line says neither, so it's dropped.

## The Codex CLI

- `codex_exec_success.jsonl`: the JSON Lines `codex exec --json` (codex-cli 0.159.2) printed for a one-word answer with `--output-schema`, the tool features turned off and `model_instructions_file` set to one line. The thread id is a placeholder. It used 4,376 input tokens.
- `codex_exec_bad_model.jsonl`: what the same command printed for `-m gpt-nonexistent-9`, which exited with status 1. The `turn.failed` message is the API's error body as a JSON string, with `status` 400. The `item.completed` of type `error` before it is only a warning.
- `codex_exec_signed_out.jsonl`: the same call with `CODEX_HOME` pointing at an empty folder. Codex retried over WebSockets, fell back to HTTPS and retried again before `turn.failed`; the capture keeps one retry of each kind, the last error and `turn.failed`. The `cf-ray`, request and thread ids are placeholders.
- `codex_exec_usage_limit.jsonl`: made by hand for a spent ChatGPT plan, from the strings in the codex-cli 0.159.2 binary ("You've hit your usage limit.", "Visit https://chatgpt.com/codex/settings/usage to purchase more credits", " or try again at "), as for a Plus plan.
- `codex_debug_models.json`: four models from `codex debug models` (three listed, one hidden), each cut to the fields the parser reads plus a few others. The real catalogue's `base_instructions` here say "(cut)".
- `codex_app_server_rate_limits.json`: the reply of `codex app-server` to `account/rateLimits/read` for a ChatGPT Plus account, with the account id removed. `primary` is the 5-hour window and `secondary` the week; `usedPercent` counts up.
- `codex_app_server_signed_out.json`: the JSON-RPC error the same request got at once when signed out.

## APIs

- `anthropic_api_success.json`: a Messages API reply with structured output, written from the documented shape. Its `usage` wrote 12,000 input tokens to the 5-minute cache and read 20,000, so the pricing test covers both.
- `openai_key_success.json`: a Chat Completions reply with structured output, written from OpenAI's documented shape. It ran at Flex (`service_tier`), names a dated snapshot as its `model`, read 1,792 of its 2,100 input tokens from the cache, and has a null where the strict schema made an optional property required.
- `openai_key_bad_key.json`: what `POST https://api.openai.com/v1/chat/completions` answered for a made-up key (`sk-proj-upnext-not-a-real-key-000000`), with status 401. It's unchanged, since OpenAI masks the key itself.
- `openai_models.json`: a `GET /v1/models` reply in the documented shape, with the models on OpenAI's pricing page, a dated snapshot, and audio, image, embedding and older models the parser leaves out.
- `openai_vllm_success.json`: a chat-completions reply from vLLM to the same prompt with `response_format: json_schema`. The `id` is a placeholder, and `prompt_text` and `prompt_token_ids`, which echo the prompt, were removed. `usage.prompt_tokens_details` is null, because this server reports no cached tokens.
- `digest_reply.json`: a model's reply to the review digest prompt, in the shape the schema asks for, with placeholder ids the test fills in. It has values outside the vocabulary, mixed case, too many aspects and moods, lists that aren't lists, an item for a game that wasn't asked about, and an item with no digest.

## Price pages

Captured and trimmed, for the tests that read price pages:

- `../prices/anthropic_pricing.md`: the start of `https://platform.claude.com/docs/en/about-claude/pricing.md` up to the end of the "Model pricing" section, then the start of its "Batch processing" section, which the parser must not read. Two rows were brought up to a later version of the page: Claude Sonnet 5.5's cache-hit price and Claude Haiku 5.5's two rows. The footnote under the table has the earlier text.
- `../prices/openai_pricing.md`: the start of `https://developers.openai.com/api/docs/pricing.md`, its "Standard pricing data" table cut after 15 models, then the start of the "Batch pricing data" table, then the first seven rows of the "Flex pricing data" table with the lines before its heading, which the page puts further down.
- `../prices/openrouter_models.json`: five models from `https://openrouter.ai/api/v1/models`, each cut to its `id`, `name`, `created`, `context_length` and `pricing`.
