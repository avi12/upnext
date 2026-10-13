# public_url and proxies

[API index](../API.md) · [Who can use the API](access.md)

## public_url

`public_url` (setting, or `UPNEXT_PUBLIC_URL`) is the address other devices use, such as `http://upnext.lan:8787` or `https://upnext.example.org`. It's saved as `scheme://host[:port]`, lowercase, without a default port or a trailing `/`; a path is refused. `GET /api/settings` gives it with `public_url_source`.

Steam sign-in (`GET /api/steam/login`) sends the browser back to it when the request was addressed to it, else to the host the request named: over `https://` when the request came over the built-in HTTPS (`tls_cert` and `tls_key`; the sign-in's cookie is `Secure` then) and `http://` otherwise. A pairing's address and QR code use it too.

## A proxy in front

A proxy in front (Caddy, Traefik, `tailscale serve`) must pass the `Host` header through, which those do by default. nginx rewrites `Host` to the upstream's address unless told not to: add `proxy_set_header Host $host;`.

A request that comes from loopback with a proxy's headers, or over HTTP/1.0 (what nginx speaks to its upstream unless `proxy_http_version 1.1;` is set, even with no headers added), is never this computer's, whatever its `Host`. Such a proxy can't make its visitors this computer: they get `401`, saying the request came through a proxy, and the server logs a warning once. A proxy that speaks HTTP/1.1, sends none of those headers and rewrites `Host` to a loopback address can't be told from a browser on this computer. When the proxy serves HTTPS and `public_url` is its `https://` address, a pairing's cookie is `Secure`.

## Trusted proxies

`trusted_proxies` (setting, or `UPNEXT_TRUSTED_PROXIES`) lists the addresses and ranges of proxies whose forwarding headers UpNext believes: `127.0.0.1`, `10.0.0.2`, `172.16.0.0/12`, comma- or space-separated, IPv6 too. It's empty by default, which changes nothing. Ranges wider than a `/8` (IPv6 `/32`) are refused. Only a request from a listed address that carries a proxy's headers is affected; a request with none of them is judged as usual, wherever it comes from. From a listed address:

- **The client's address** is the right-most hop in `Forwarded`'s `for` or in `X-Forwarded-For` that isn't itself listed. A hop whose address can't be read (`unknown`, a hidden name) counts as a public client, never as a proxy to skip. So does a request from a listed address that is marked as a proxy's but names no client at all: one carrying only `X-Real-IP` (never read), `Via` or `X-Forwarded-Proto`, or speaking HTTP/1.0 with no forwarding header. The client's address is used for the request's log line ("… for 192.168.0.42"), the limits, and whether plain HTTP is allowed: a public client over plain HTTP is refused a token, as if it had connected itself.
- **HTTPS and the host** are what that hop's proxy recorded: the same `Forwarded` element's `proto` and `host`, or the `X-Forwarded-Proto` and `X-Forwarded-Host` value at the same place in its list (when the list is as long as `X-Forwarded-For`), else the list's right-most value, the one the proxy in front of UpNext wrote. So the host check and `public_url` come out right behind a proxy that rewrites `Host`, and what a client wrote into the headers itself is never taken over its proxy's word.
- **Both kinds of header** in one request (`Forwarded` and `X-Forwarded-*`) must name the same client, proto and host, else `400`: behind a proxy that sets one kind, a visitor could otherwise say what they liked with the other. A proxy in front of UpNext should replace or append to the kind it sets.
- **No header names a player.** `Remote-User`, `X-Forwarded-User`, `Tailscale-User-Login` and the like are ignored. Proxies decide access only; identity comes from the identity provider's sign-in alone.

From any other address, those headers mean what they always do: another device, which needs a token, and from loopback, "this request came through a proxy".

## The proxy does the login

`trusted_proxy_login` (setting, or `UPNEXT_TRUSTED_PROXY_LOGIN`; off by default): with it on, a request from a listed proxy needs no token. It lands on "Who's playing?" like a browser on this computer, with PINs as usual, and the choice is kept in that browser's player cookie.

It isn't this computer: nothing on the [only on this computer](local-only.md) list, and nothing a `full` token can't do. Everything else an unbound `full` caller does, it does, as someone at the keyboard can: choose any player with the `UpNext-Player` header (PINs as usual), list and revoke tokens and devices, and read the export. It must be addressed to `public_url` or, when the proxy rewrites `Host` to its upstream's, a loopback name (`403` otherwise, which would be DNS rebinding through the proxy's address). A write must carry an `Origin` or `Sec-Fetch-Site: same-origin`, since the proxy's own sign-in cookie rides along on any request.

It trusts everything that can reach UpNext from a listed address, and the network between the proxy and UpNext. Use it only with a proxy that really makes people log in (Authelia, Authentik, oauth2-proxy) or a tailnet (`tailscale serve`), with UpNext reachable only through it. Both settings are set only on this computer or in the environment, and `GET /api/settings` gives them with their `_source`.
