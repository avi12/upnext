# Another provider: OpenAI-compatible servers

[API index](../API.md) · [AI backends](ai-backends.md) · [OpenAI API key](ai-openai-key.md)

`llm_backend` `openai` is any server that speaks `POST {base}/chat/completions`: Ollama, LM Studio, llama.cpp, vLLM, LocalAI, or a hosted OpenAI-compatible API. Settings calls it "Another provider". An `api.openai.com` address works here as "Other server", but the [OpenAI API key](ai-openai-key.md) backend is the one made for it.

- `openai_base_url` (`http://localhost:11434/v1`, Ollama), `openai_model` (required; the server's model id), `openai_api_key` (optional and write-only; `GET` gives `openai_api_key_set`), and `openai_digest_model`, a cheaper model on the same server for review summaries, which falls back to `openai_model`. Env `UPNEXT_OPENAI_BASE_URL`, `UPNEXT_OPENAI_MODEL` and `UPNEXT_OPENAI_API_KEY`. The Claude model settings don't apply.
- From another device, a `PUT` that changes `openai_base_url` clears the saved key unless it sends a new one, so a token can't send the key to an address of its choosing. With the key in the environment, such a change is `403` unless it brings its own key.
- UpNext asks for JSON that fits a schema (`response_format: {type: "json_schema"}`). When the server answers `400` or `422` to that, it tries again with `{type: "json_object"}` and the schema in the system prompt, and it accepts replies wrapped in code fences. An error from the server shows its API error message (`error.message`), never the page it answered with.
- `openai_thinking` (`UPNEXT_OPENAI_THINKING`): `auto` (the default), `off` or `on`. With `off`, each request carries `"chat_template_kwargs": {"enable_thinking": false}`, which vLLM and llama.cpp pass to the model's chat template, so a model that reasons before it answers (Qwen) answers at once. `on` sends nothing and leaves the model's default. `auto` is `off` for a server on this computer or network (loopback, private, tailnet and link-local addresses, or a name with no dot or ending `.lan`, `.local`, `.home`, `.internal`, `.home.arpa` or `.ts.net`) and `on` elsewhere, since a hosted API may refuse a field it doesn't know.

## GET /api/llm/models

`{"ok": true, "models": ["..."]}` from the server's `GET {base}/models`, or `{"ok": false, "models": [], "error": "..."}`. Settings uses it to suggest model ids.

## Prices

Calls to `api.openai.com` are priced and charged at OpenAI's list prices, and calls to `openrouter.ai` at OpenRouter's. Any other server has no price figure ([Prices](ai-prices.md)).
