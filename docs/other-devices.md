# Other devices and scripts

The computer UpNext runs on needs nothing. Other devices are kept out until you let them in:

1. **Allow other devices** (Settings › Devices and API) makes UpNext listen on your network as well as on `127.0.0.1`. It's off by default.
2. **A phone or another browser asks, and you allow it.** Open UpNext's address on the phone and choose **Ask to be allowed**. Every UpNext window and browser on the computer it runs on shows "A phone called … wants to use UpNext" with a four-digit number. Check that the phone shows the same number and choose **Allow** (with two or more players, choose whose phone it is first). `upnext pair requests` and `upnext pair approve 4821` do the same in a terminal. A request lasts two minutes, and an address denied three times waits ten minutes before it can ask again.
3. **Or type a code.** **Add a device** shows a six-digit code for five minutes, with a QR code of the address; scanning it pairs with nothing to type. Either way the browser keeps a cookie and shows in the token list as a paired device you can revoke. The [Android app](install/android.md) finds the computer on the network and asks the same way.
4. **A script uses a token.** Make one in Settings, or with `upnext token create --name x --scope read|full`: `read` looks and changes nothing, `full` does everything. Send it as `Authorization: Bearer upnext_…`, for example `curl -H "Authorization: Bearer $TOKEN" http://upnext.lan:8787/api/v1/play-later`.

The API is versioned under `/api/v1/`. [The HTTP API](API.md) is the guide, and the OpenAPI document is at `/api/v1/openapi.json`.

## HTTPS and proxies

Over plain HTTP, UpNext takes a token, cookie or pairing code only from a private address: your network, this computer, or Tailscale's `100.64.0.0/10`. For anything further, use HTTPS. Either give UpNext a certificate (Settings, HTTPS, or `UPNEXT_TLS_CERT` and `UPNEXT_TLS_KEY`; HTTPS then works on the same port), or put a proxy in front.

`tailscale serve --bg 8787` is the easiest proxy. Caddy, Traefik and others work too; keep the request's `Host` (nginx: `proxy_set_header Host $host;`). Then set "Address for other devices" (`public_url`) to the address they use.

A request a proxy forwards carries the proxy's headers (`X-Forwarded-For` and the like) or, from nginx by default, speaks HTTP/1.0, so UpNext asks it for a token like any other device's. The one setup it can't tell from this computer is a proxy that speaks HTTP/1.1, strips every forwarding header and rewrites `Host` to `127.0.0.1`. Don't configure one that way.

## Found on the network

While other devices are allowed, UpNext announces itself on your home network (mDNS, `_upnext._tcp`) under the computer's name, so another computer's app window can find it. Settings › Devices and API changes the name or turns the announcement off. It carries the name, an install id, the port and the version, nothing about your library. On Windows, the firewall must let UpNext receive on UDP port 5353 for another computer to find it; the address can always be typed instead.

## The library on another computer

`upnext app --connect`, or "Is your library on another computer?" on setup's first page, lists the computers on the network that run UpNext. Choose one and it asks that computer to allow this one, as a phone does. Once someone there taps Allow, the window shows that computer's UpNext, and opens on it from then on. Games still start on the computer the window runs on. "Use this computer's library", in Settings › Devices and API, goes back. The window keeps the other computer's token in `computers.json` in UpNext's config folder, never in the library or its backups.

## Behind your own sign-in

Two optional settings in Settings › Devices and API fit UpNext into a home server's sign-in. Both are off until set, on the computer UpNext runs on or in its environment.

- **Sign in with an identity provider** (Authelia, Authentik, Keycloak, Pocket ID or any OpenID Connect provider). Register UpNext there as a client with the redirect address Settings shows (`public_url` followed by `/api/v1/oidc/callback`), then set the issuer, client id and secret (`UPNEXT_OIDC_ISSUER`, `UPNEXT_OIDC_CLIENT_ID`, `UPNEXT_OIDC_CLIENT_SECRET`). The page another device lands on then offers "Sign in with …", which pairs the browser as that account's player, and makes a player the first time an account signs in (`oidc_auto_provision`, on by default). To sign in as a player who's already here, choose **Link the next sign-in** for them in Settings › Players, sign in on their device, confirm in Settings that the account shown is theirs (with their PIN, when they have one), and sign in again. `oidc_allowed_groups` keeps it to some of the provider's groups.
- **Trusted proxies.** List the proxies in front of UpNext (`trusted_proxies`, `UPNEXT_TRUSTED_PROXIES`, as `127.0.0.1` or `10.0.0.2, 172.16.0.0/12`) and UpNext believes their `X-Forwarded-For`, `X-Forwarded-Proto` and `X-Forwarded-Host`, so the log and limits see the device behind. **The proxy does the login** (`trusted_proxy_login`) lets the visitors it lets through in without pairing, to "Who's playing?" with PINs as usual; no header from the proxy ever chooses a player. Behind nginx with Authelia's `auth_request`, list nginx's address. With `tailscale serve --bg 8787` on the computer UpNext runs on, that's `trusted_proxies = 127.0.0.1` with the switch on: everyone on your tailnet is in. Never turn the switch on for Tailscale Funnel, or any proxy that lets anyone through.

[Who can reach UpNext](security/devices.md) has the precise rules.
