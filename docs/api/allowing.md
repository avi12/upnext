# Allowing a device with a tap

[API index](../API.md) · [Tokens and pairing](pairing.md) · [Other computers](network.md)

In place of typing a code, a device can ask, and someone at the computer allows it with a tap. The code stays as the fallback ("Type a code instead") for a computer whose screen nobody sees. A phone's browser asks from the pair page; the Android app and the app window on another computer ask after finding the computer on the network.

## POST /api/pair/request

`{"name": "Pixel 9", "kind": "phone", "wants": "cookie"}`, all optional: `name` from the browser as for `POST /api/pair`; `kind` one of `phone`, `tablet`, `tv`, `computer`, from the browser; `wants` `cookie` for a browser or WebView, `token` for an app that keeps the token itself. Answers `201`:

```json
{"id": "9c1e…", "secret": "…", "match_code": "4821", "expires_at": 1790000120, "computer": "den", "computer_id": "4f2a9c01d3e85b67"}
```

The device shows the four-digit `match_code`, and the computer shows it too, so nobody allows a neighbour's request by mistake.

Needs no token, but only from a private address (`403` from any other, HTTPS included): the client's own, or, through a proxy listed in `trusted_proxies` ([Proxies](proxies.md)), the client's as the proxy names it. Through a proxy that isn't listed (any request with a proxy's headers, or HTTP/1.0, from an address not in the list) it's `403` whatever the address, since UpNext can't tell who is asking; pair with a code instead, or list the proxy.

A request waits two minutes. One waits per address: asking again replaces it, ten seconds on at the earliest (`429` with `Retry-After` before that), and the ten seconds count an IPv6 address by its /64, since one device can pick many addresses in it. Two wait at most per IPv6 /64, and three at most whatever the addresses (`429`, `Retry-After: 30`). After three denials an address waits ten minutes (`429` with `Retry-After`). The window is asked for attention at most every ten seconds, and not again for a request that replaces one. Each request is logged with where it came from and how to answer it without a screen: `upnext pair approve 4821`, or `docker exec upnext upnext pair approve 4821` in the container.

## GET /api/pair/request/{id}

With the secret in `UpNext-Pair-Secret`. Answers `{"status": "waiting"}`, `{"status": "denied", …}` or `{"status": "allowed", "id": 9, "name": "Pixel 9", "token": null, "address": null}`. Allowed, a request that wanted a cookie gets the `upnext_device_<port>` cookie, as `POST /api/pair` gives it; one that wanted a token gets it in `token`, the only time it's seen. `address` is `public_url` when it's set: a token is then taken only at that name. An answer is collected once; then the request is gone (`404`, as for an unknown id, a wrong secret or one that expired). Needs no token; only from a private address, by the same rule as asking, and, like `POST /api/pair`, not over plain HTTP from a public one.

## GET /api/pairing/requests

On this computer only. The requests waiting, oldest first, for the dialog every UpNext window and browser on the computer shows ("A phone called "Pixel 9" wants to use UpNext. It shows 4821."):

```json
{"requests": [{"id": "9c1e…", "name": "Pixel 9", "kind": "phone", "match_code": "4821", "from": "192.168.0.42", "via": null, "expires_at": 1790000120}], "players": []}
```

`from` is the address it asked from, the client's as a listed proxy names it; `via` is that proxy's address, or null when it asked directly. The dialog shows both ("From 100.64.0.7 through the proxy at 127.0.0.1 on your network"). `players` lists `{"id", "name"}` when there are two or more players, for the dialog's "Whose device is it?"; it's empty with one.

## POST /api/pairing/requests/{id}

On this computer only. `{"allow": true, "player_id": 2}` → `204`. Allowed, the device gets a device token of `full` scope when it collects, named as it asked (with ` 2`… when taken). With `player_id` it's bound to that player, and needs `"pin"` when that player has one; null or left out makes it the household's, like a paired browser's. `{"allow": false}` denies it. `404` when it isn't waiting any more, `400` for an unknown player.

## GET /api/pair/open

With `Authorization: Bearer upnext_…`: `303` to `/`, with that token as the `upnext_device_<port>` cookie. The app window on another computer opens this first, since a webview sends a header only with the first page it loads, and the page's own requests carry the cookie from then on. On this computer, with no token, it only goes to `/`.
