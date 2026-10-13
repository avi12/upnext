# Sending verdicts

[API index](../API.md) · [Your verdicts](verdicts.md) · [From others](community-verdicts.md)

These talk to the verdicts service at `verdicts_url` with the player's key ([Your verdicts](verdicts.md)).

## POST /api/verdicts/{title_id}/send

The answers. Checks them as the service does (`400` with `problems`), needs the consent accepted (`409`) and a service address (`503`), makes the player's key if they have none, and sends the verdict with only the allowlisted fields and the key in `Authorization`.

- `200` `{"verdict": MyVerdict, "message"}` when the service took it (`sent`, or `held`). Sending again replaces it.
- `202` when the service answered `429` or couldn't be reached: the verdict is `queued` and sent again after `Retry-After` (at most a day; an hour when it doesn't say) by the server's scheduler, for up to a week.
- A refusal leaves a sent verdict as it was and keeps the answers as a draft with `error`: `400` with the service's `problems`, `403` when the service blocked the key, `502` for anything the form can't fix.

## Taking verdicts down

- `DELETE /api/verdicts/{title_id}`: the service leaves it out of the next daily snapshot and deletes it within a week. `{"verdict": MyVerdict}` with `state` `withdrawn` (`removed` when the service no longer had it), or `{"verdict": null}` for a draft or queued verdict never sent, which is just forgotten. `404` for no verdict of yours about the game; `409` when it's down already or the key that sent it is gone; `502` when the service couldn't be reached (nothing changed).
- `POST /api/verdicts/withdraw-all`: "Take down all my verdicts and forget my key". Withdraws every verdict of the player's key, then forgets the key; queued verdicts go back to being drafts, so nothing goes out under a new key unasked. `{"withdrawn": 12, "forgot_key": true}`; `502` when the service couldn't be reached (nothing changed, key kept).

## POST /api/verdicts/refresh

Asks the service which verdicts the player's key sent and brings this computer's list up to date: verdicts it lost (a reset, or an older backup restored) come back, matched by Steam appid, then GOG id, and held or removed ones say so; a queued change is left alone. `{"listed", "added", "changed", "unmatched"}`, where `unmatched` counts verdicts about games not in this library. With no key, `listed` is 0.

## POST /api/verdicts/reports

`{"verdict_id": "…", "reason": "spam"}`, the reason being `spam`, `personal`, `abuse` or `off_topic`. Passed on to the service with the player's key (made now if they have none), so the consent must be accepted first (`409`). `202` `{}`; `400` for an unknown reason or an id that isn't one; `404` for a verdict that isn't published; `429` when the day's reports are used up.
