# AI accounts

[API index](../API.md) · [Players](players.md) · [AI backends](ai-backends.md)

Like [store accounts](accounts.md), these act for the request's player, are `403` while nobody has been chosen, and take the `UpNext-Player` header. None returns a key's value.

An **AiAccount** is one backend with its models and keys: `id`; `owner_id` and `owner`; `backend` (one of `llm_backends`, or null for the environment's or the built-in one); `settings` (the models, address, effort and thinking settings); `keys_set` (the names of the keys it holds, such as `anthropic_api_key`, sorted); `sharing` (`everyone`, `chosen` or `owner`); `offered_to` (player ids, for its owner with `chosen` sharing, otherwise `[]`); `mine`; `in_use` (the asker's picks run on it); `library` (the library's own AI work runs on it); and `this_computer` (the first account, which the environment's AI settings and this computer's signed-in Claude and Codex CLIs belong to).

- `GET /api/ai-accounts` lists every AI account this player owns or is offered, theirs first.
- `POST /api/ai-accounts` `{"backend", "settings", "sharing", "offered_to", "library"}` answers `201` with the account. `settings` are AI settings and keys as `PUT /api/settings` takes them. `sharing` is `everyone` unless `offered_to` is sent without it, which makes it `chosen`. A Claude Code CLI account other than this computer's needs its own token (`claude_oauth_token`, from `claude setup-token`); a Codex CLI one signs in in its own folder (`CODEX_HOME=<data folder>/codex/<id> codex login`). A player with no AI account starts using it. `400` for an unknown backend, a key that isn't an AI setting, a bad value or `sharing`, or an `offered_to` id that's the owner's or no player's; `403` for `"library": true` from a token bound to a player.
- `PUT /api/ai-accounts/{id}` takes the same body, each field optional (null or `""` clears a setting). `"library": true` runs the library's own AI work (summaries, series guides, hidden gems) on it, and `false` changes nothing; a bound token can't send `library` at all. From another device, a new `openai_base_url` clears the account's `openai_api_key` unless the same request sends one. `400` as for `POST`; `403` for an account that isn't theirs, or for `library` from a bound token; `404` for no such account.
- `DELETE /api/ai-accounts/{id}` answers `204`. Whoever used it gets picks without AI until they choose another, and the library's AI work stops until one is chosen for it.
- `PUT /api/ai-accounts/{id}/use` (no body) makes it the AI this player's picks, Help me decide and Second Chance run on. `403` for one that isn't theirs and isn't offered to them; `404` for no such account.

`GET /api/ai/plan` reads the request player's AI account: each Claude Code CLI or Codex CLI account with its own sign-in, and its readings are its own. A Claude Code CLI account other than this computer's with no token of its own has no bars to read (`subscription: false`).
