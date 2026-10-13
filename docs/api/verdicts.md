# Backlog verdicts: your own

[API index](../API.md) · [Sending verdicts](verdicts-sending.md) · [From others](community-verdicts.md) · [The verdicts service](../VERDICTS.md)

A backlog verdict is your answer, for other players, about how a game fits a backlog and limited time. [VERDICTS.md](../VERDICTS.md) has the questions, the checks and the service. These routes are your own verdicts: the form, drafts, the consent screen, sending, taking down and reporting.

The service's address is the `verdicts_url` setting (`UPNEXT_VERDICTS_URL`), `https://verdicts.upnext.games` unless set; nothing goes there until a player sends a verdict or turns on verdicts from other players. It's set only on this computer, since each player's key goes there, and only this computer talks to the service: a phone or another browser sends through it.

Refusals are `{"error": "…", "problems": [{"field": "text", "why": "has an email address; take it out to send"}]}`, with `problems` when there's something to fix field by field. Writes need a `full` token, and a body over 16 KB is refused (`413`).

## GET /api/verdicts

`{"service": true, "has_key": true, "consent": {…}, "verdicts": [MyVerdict]}`, newest change first. `has_key` says the player has a key (made the first time they send or report); the key itself never comes back from any route.

A **MyVerdict** is `{"title_id", "name", "answers", "steam_appid", "gog_id", "remote_id", "state", "retry_at", "error", "sent_at", "updated_at"}`. `answers` holds `outcome`, `worth`, `hours`, `clicked`, `sessions`, `limited_time`, `factors`, `stop_point`, `stop_note`, `stopped_for` and `text`. `state` is:

- `draft`: saved here and not sent (with a `remote_id`, changes not sent to a verdict the service has as it was);
- `queued`: the service was busy or out of reach, and UpNext sends it at `retry_at` while it runs, for up to a week, then makes it a draft again;
- `sent`; `held` (the service holds it while the game gets a rush of verdicts); `withdrawn`; or `removed` (by the service's moderators).

`remote_id` is the id the snapshot publishes it under, and `error` why the last try didn't go through.

## GET /api/verdicts/vocab

The contract as `docs/verdicts-contract.json` holds it: the questions and their answers with their words, the limits, the text checks the form runs as you type, the consent text and the report reasons.

## GET and POST /api/verdicts/consent

`{"version": 1, "title", "paragraphs": […], "accept", "decline", "text_box_line", "licence": "CC0-1.0", "licence_url", "terms_url", "accepted", "accepted_on", "current"}`. `terms_url` is the service's `/terms`, or null with no service. `POST` with `{"version": 1}` accepts that version (the current one; `400` for another) and sends nothing; it's kept per player with the day. A new consent text is a new version, and sending asks again.

## GET /api/verdicts/draft/{title_id}

The form for a game: `{"title_id", "name", "cant_send", "answers", "from", "problems", "mine", "outdated", "service", "consent"}`. `answers` are your saved or sent ones, or else what UpNext fills in from your mark, playtime, play diary and stop reasons, with `from` naming where each came from (`mark`, `playing`, `playtime`, `diary`, `stop_reasons`, `no_ending`). `problems` is what's left before it can be sent. `cant_send` says why a game with neither a Steam appid nor a GOG id can't get one. `mine` is your MyVerdict, or null. `outdated` is true when a sent verdict's outcome no longer matches your mark. `404` for no such title.

## PUT /api/verdicts/{title_id}

The answers (unknown fields are refused), saved as a draft on this computer. Answers with the MyVerdict. A blank answer or text is no answer. `400` for a game that can't get a verdict; `409` for a verdict that's sent (send it again to replace it).

Sending, taking down, refreshing and reporting are on [Sending verdicts](verdicts-sending.md).

## Settings

`verdicts_url` (the project's service unless set, with `verdicts_url_source` `default`; only on this computer, and `https://`, or `http://` only to `localhost`, `127.0.0.1` or `[::1]`, as [from others](community-verdicts.md#jobs-and-settings) says) and `verdicts_ask` (`true`: offer "Tell other backlog players how it went?" after a mark that ends something; each player's own).

Each player's key is kept in the database where no settings route reads it. It's in backups, left out of a copy without keys, and kept from this install when a restored copy has none for that player. Merging titles keeps the surviving title's verdict, and clearing history in setup's reset clears verdicts here (the service keeps what was sent, and `refresh` lists it again). `upnext verdicts list`, `send <game>`, `withdraw <game>`, `withdraw --all` and `refresh` do the same from a terminal; `send` shows the verdict and, the first time, the consent text, and asks (`--yes` only once the consent was accepted).
