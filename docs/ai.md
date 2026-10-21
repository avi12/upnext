# Choosing an AI

Picks, review summaries, series guides, second chances and hidden gems use an AI. Choose it in Settings › Picks, "AI", or with `UPNEXT_LLM`:

- **`claude-cli`** (the default): `claude -p` on your Claude plan, through the [Claude Code](https://claude.ai/code) command-line tool you installed and signed in to.
- **`codex-cli`**: `codex exec` on your ChatGPT plan. UpNext turns off Codex's tools, plugins and your config for each call. The models default to the newest Luna your account has.
- **`api-key`**: the Anthropic API with your key (`ANTHROPIC_API_KEY`), billed per token. `sonnet`, `opus` and `haiku` mean `claude-sonnet-5-5`, `claude-opus-5-5` and `claude-haiku-5-5`, and full model ids work too.
- **`openai-key`**: the OpenAI API with your key (`OPENAI_API_KEY`). Summaries and guides use OpenAI's cheaper Flex tier when it has room.
- **`openai`**: any OpenAI-compatible server, such as Ollama, LM Studio, llama.cpp, vLLM or OpenRouter. Give its address, model and optional key in Settings or with `UPNEXT_OPENAI_*`. A model that thinks before it answers (Qwen on vLLM) is asked not to when the server is on your own computer or network, which makes picks much quicker; Settings › Picks' Thinking choice changes that.
- **`copilot`** (Windows only): Microsoft Copilot's chat on copilot.com, as the personal Microsoft account Windows is signed in to: no key. It needs Microsoft's `msalruntime.dll` to get a token, which Git for Windows ships (or set `UPNEXT_MSALRUNTIME` to its path). Copilot takes no JSON schema, so UpNext asks for JSON in the prompt, and replies are less reliable than the other backends'; it reports no tokens and has no model choice. The free account gets 50 messages a day, each call is one, and each leaves a chat in your Copilot history. Microsoft's terms say not to use programs to access Copilot ([Third-party services](THIRD_PARTY.md)), so choose it knowing that.

With the Claude Code or Codex CLI, UpNext runs the program on your own plan and never reads its credentials. With a local server such as Ollama, nothing leaves your machine.

## Models and cost

Picks and review summaries default to Sonnet on the Claude CLI. With an API key, picks default to Sonnet 5.5 and review summaries to Haiku 5.5, which writes them nearly as well at about a twentieth of the price.

A pick's prompt is about 27k input tokens. Asking the same request again on the same day reuses most of it from the provider's prompt cache. Settings › AI usage shows every call's tokens by day, task and model, and, if you like, what they cost at list prices; on a subscription that's what the same calls would cost on the API. On a subscription it also shows the plan's usage bars, read in ways that spend nothing.

Review summaries, second chances and hidden gems can run on a second AI, such as a model at home, while picks stay on your subscription (Settings › Picks, "Summaries").

## What runs on its own

The AI jobs (review summaries, series guides, second chances, hidden gems) are off until you turn them on in Settings › Jobs or the setup wizard, which shows an estimate of what a first run takes. Picks work without any AI, from UpNext's own ranking.

To see exactly what a pick sends, `upnext prompt "…"` prints the prompt without sending it. [Privacy](privacy.md) lists what each AI task is given.
