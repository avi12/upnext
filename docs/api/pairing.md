# Tokens and pairing

[API index](../API.md) · [Who can use the API](access.md) · [Allowing a device](allowing.md)

## Tokens

Only a token's SHA-256 is stored; the token is in the answer that makes it and nowhere else. Tokens are `upnext_` and 40 letters and digits. Backups keep them, and a download or export without keys leaves them out. A restore or an import always keeps this install's tokens in place of the copy's, so a token revoked since the backup stays revoked and a device paired since keeps working.

### GET /api/tokens

Needs `full` from another device.

```json
{ "tokens": [ { "id": 1, "name": "phone", "scope": "read", "kind": "token", "hint": "upnext_Ab12",
                "created_at": 1790000000, "last_used_at": 1790000300, "player_id": null } ],
  "current": null }
```

- `kind` is `token` (made by hand), `device` (a paired browser) or `oidc` (a browser signed in with the identity provider).
- `last_used_at` is written at most once a minute.
- `player_id` is the player a token or device is bound to and always acts for. When it's null, the request uses the `UpNext-Player` header or the default player.
- `current` is the id of the token the request came with (null from this computer). A token bound to a player gets only itself in `tokens`.

### POST /api/tokens

`{"name": "phone", "scope": "read", "player_id": 2}` makes one, on this computer only: `201` with the row above plus `"token": "upnext_…"`. `player_id` binds it to that player (left out or null: not bound; `400` for an unknown player), with `"pin"` when that player has one (`403` when it's missing or wrong, `429` while locked). A name is 1 to 60 characters and unique (`400` otherwise). Control characters and the marks that reverse text are taken out of a token's or a pairing browser's name, and runs of spaces become one.

### DELETE /api/tokens/{id}

Revokes one at once: `204`, or `404`. A token bound to a player can revoke only itself (`403` for another).

From a terminal: `upnext token create --name phone --scope read` prints the token alone on stdout; `upnext token list`; `upnext token revoke <id or name>`.

## Pairing a browser with a code

Settings › Devices and API, "Add a device", on the computer UpNext runs on, shows a six-digit code for five minutes, with a QR code of the address. The new device types it and gets a device token of `full` scope as a cookie (`upnext_device_<port>`, `HttpOnly`, `SameSite=Strict`, `Secure` over HTTPS). The device is then in the token list (`kind: "device"`) and can be revoked like any token.

### GET /api/pairing

On this computer only, since a device that could read a waiting code could pair a second device. Answers the waiting code or `null`:

```json
{"code": "042917", "expires_at": 1790000300, "address": "http://192.168.0.42:8787", "qr_svg": "<svg…"}
```

`address` is `public_url`, else this computer's address on its network; null, with no QR code, while UpNext only listens on this computer. The QR code opens `address` with the code in its fragment (`http://192.168.0.42:8787/#pair=042917`): the UI fills the code in and pairs, then takes it out of the address. A fragment never reaches the server or a proxy's log.

### POST /api/pairing

A new code (`201`, the shape above), on this computer only; any earlier code ends. `?then=setup` makes the QR code open the setup wizard instead of the first page (`http://192.168.0.42:8787/setup#pair=042917`): big screen's first run hands the steps that need typing to a phone this way. `then` is `home` (the default) or `setup`; anything else is a `400`. `?player=<id>|shared` says whose device it will be ([Players](players.md)).

### GET /api/pairing/outcome?code=042917

On this computer only: what became of a code shown here. `{"outcome": "waiting", "device": null}` while it can still be typed; `{"outcome": "paired", "device": {"id": 7, "name": "Android phone"}}` once a device used it; `{"outcome": "ended", "device": null}` when it ran out, took five wrong tries, or was ended or replaced. A used code and an expired one both leave `GET /api/pairing` at `null`, so a page waiting on its code asks this. Only the last code that paired a device is remembered, until UpNext restarts.

### DELETE /api/pairing

Ends the code: `204`, on this computer only.

### POST /api/pair

`{"code": "042917", "name": "phone"}` → `201` `{"id": 7, "name": "phone"}` and the cookie. `name` is optional: when left out it's worked out from the browser ("Android phone"), with ` 2`, ` 3`… when taken. Needs no token, but not over plain HTTP from a public address (`403`). A wrong code is `401`, with the tries left; five wrong tries end the code. `404` when no code is waiting. A code works once.

`POST /api/pair` isn't rate-limited (whoever calls it has no token to count against), so anyone who can reach the server can end a waiting code with five wrong guesses. That stops a pairing; it doesn't get past one. Make a new code and pair again.

With `"wants": "token"`, for an app that keeps the token itself (the Android app pairing from a QR code or a typed code), no cookie is set. The answer adds the token, the only time it's seen, and what the app needs to find the computer again: `{"id": 7, "name": "Pixel 9", "token": "upnext_…", "computer": "den", "computer_id": "4f2a9c01d3e85b67"}`. When `public_url` is set, `address` is added and is that address: a token is then taken only at that name. `wants` is `cookie` (the default) or `token`.
