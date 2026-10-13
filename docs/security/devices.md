# Who can reach UpNext

Other devices are kept out until you let them in. [The HTTP API's access rules](../api/access.md) have the precise behaviour; this page says what each rule protects.

## Tokens and pairing

- **Allow other devices** (Settings › Devices and API) makes the server listen on your network as well. It's off by default. The container image turns it on, since a container's port is reachable only through what you publish.
- Every request that isn't this computer's needs `Authorization: Bearer upnext_…`, a token made in Settings or with `upnext token create`, or a paired browser's cookie. A token is `read` (looks, changes nothing) or `full`. Only a token's SHA-256 is stored, and it's shown once.
- **Pairing** takes a six-digit code that lasts five minutes and works once; five wrong tries end it. The browser gets an `HttpOnly`, `SameSite=Strict` cookie (`Secure` over HTTPS), listed with the tokens and revoked the same way. The cookie's name carries the port the server listens on, since browsers send a host's cookies to every port, so two UpNexts on different ports of one host never read or clear each other's. A container listens on 8787 inside whatever port it's published on, so a container and another UpNext on 8787 on the same host share the name and unpair each other; neither is ever let in by the other's cookie, since a token works only on the install that made it.
- **Asking to be allowed** needs no code: a device on a private address asks, and someone at the computer UpNext runs on allows it, after checking that the four-digit number in UpNext's dialog (or `upnext pair requests`) is the one the device shows. Only this computer can answer. A request lasts two minutes, and the device collects the answer with a secret it got when it asked, so nobody else can; the token or cookie is made only then. Three requests wait at most, one per address. An address asks again ten seconds on at the earliest, and one denied three times waits ten minutes. One device can pick many IPv6 addresses in its /64, so the ten seconds count a /64 as one address and two requests at most wait from one /64. Allow takes a second to become pressable each time another request is shown, so a click meant for one never allows the next.
- **Through a proxy**, a device may ask only when the proxy is listed in trusted proxies ([Sign-in and proxies](sign-in.md)) and names a client on a private network, such as a tailnet device through `tailscale serve`; the dialog shows the client's address and the proxy's. Through a proxy that isn't listed nobody may ask, since anyone it lets in could be behind it: such a device pairs with a code.
- With other devices allowed, UpNext announces its name, an install id, its port and its version on the local network (mDNS). The announcement can be turned off.

## Plain HTTP only on private networks

Over `http://`, a token, cookie or pairing code is accepted only from loopback, RFC 1918, link-local, Tailscale's `100.64.0.0/10` and IPv6 unique-local or link-local addresses. From anywhere else UpNext answers `403` and says to use HTTPS: its built-in HTTPS (your certificate and key) or a proxy in front.

## Phones and other computers

The Android app and the app window on another computer pair the same way and keep their tokens apart from the library. [Phones and other computers](apps.md) says how each guards its token.

## Only on this computer

Whatever the token, and also through a proxy on this computer, these answer only on the computer UpNext runs on: making tokens and pairing codes, reading a waiting code, and answering a device that asks to be allowed, so a token or a paired phone can't hand out more access; the other computers the app window uses (their tokens are kept here); downloading, restoring, importing and deleting backups; clearing things before the setup wizard starts again; opening the GOG login window; settings that name files on the server (the Galaxy database, the certificate and key) and a sync's own Galaxy path; and how many backups are kept, since keeping fewer removes the rest.

**One exception: the Galaxy upload.** A full token, a paired browser's included, may upload a copy of GOG Galaxy's `galaxy-2.0.db` (`POST /api/galaxy/upload`), so setup can take it from the computer Galaxy is on. The write is narrow: the file must open as SQLite with Galaxy's tables and is at most 1 GB; it always lands under the one fixed name `galaxy-2.0.db` beside UpNext's database, replacing an earlier upload and never another file; the work file is removed however the upload ends; one upload runs at a time; and `galaxy_db` is set to that file, not to a path the request names. A read token can't upload it.

## Limits

Each token gets about 10 requests a second, one pick and one Help me decide at a time. This computer has no limit.
