# Sign-in and proxies

## Signing in with an identity provider

Off until an issuer, a client id and `public_url` are set, on this computer or in the environment. UpNext is the relying party for the authorization code flow with PKCE (S256), with a state that works once and lasts ten minutes, tied to the browser by a cookie, and a nonce.

It checks the ID token itself: a signature by one of the keys the provider publishes (RSA, ECDSA or Ed25519; never `none`, and never one signed with the client secret), the issuer the provider's discovery names (which must be the issuer set), the audience and `azp`, the expiry, `iat` and `nbf` with two minutes' leeway, and the nonce.

Starting a sign-in needs no token, so it is limited: three wait at most from one address (an IPv6 /64), a fourth pushing out that address's own oldest, and 32 in all, after which a new one is refused rather than pushing out another device's. The way back ends only the sign-in its state names. That leaves a known gap, accepted as the price of never pushing out another device's sign-in: about 11 addresses (or /64s), each holding three sign-ins it never finishes, can keep anyone new from starting one for the ten minutes they last.

A signed-in browser gets a device token bound to the account's player, as a paired phone does, with the same cookie and the same revoking; signing out revokes it. An account is its linked player:

- **Link the next sign-in** (on this computer, with that player's PIN, for ten minutes) makes a new account that signs in wait, unlinked and with no token, until someone at this computer sees its name, username, email and subject, enters the player's PIN again and confirms it is that player. Whoever happens to sign in first is never linked by that alone.
- Otherwise, with auto-provisioning on (the default), a new account gets a new player. Accounts are never matched to players by name or email, so a provider that lets people pick their own display name can't make anyone someone else. With auto-provisioning on, anyone the provider signs in gets a player of their own, so limit it to a group (`oidc_allowed_groups`) or turn auto-provisioning off when the provider has accounts that shouldn't use UpNext.

The client secret is write-only: never sent back by the API, shown as `<set>` in logs. The provider's sign-in isn't checked again after it pairs the browser: disabling an account there doesn't sign out its browsers here. Settings › Players (Unlink) or revoking the device does.

## Trusted proxies

`trusted_proxies` lists the addresses whose forwarding headers UpNext believes, and nothing else. It's empty by default, ranges wider than /8 are refused, and a request with none of a proxy's headers is judged as before, even from a listed address.

From a listed address, the client's address is the right-most hop in `X-Forwarded-For` or `Forwarded` that isn't itself listed; one that can't be read counts as a public client. HTTPS and the host are taken as that hop's proxy recorded them. A request carrying both kinds of header must have them agree or is refused. So the plain-HTTP rule, the limits and the log apply to the client and not the proxy. No header ever names a player: `Remote-User` and the like are ignored.

With **The proxy does the login** (`trusted_proxy_login`, off by default), a listed proxy's visitors need no token and land on the player picker, PINs as usual. Each is an unbound full caller, as someone at the keyboard is: they can choose any player with the `UpNext-Player` header (PINs as usual), list and revoke tokens and paired devices, and read the export, but never do what only this computer can. That trusts everything that can connect from a listed address, so turn it on only for a proxy that makes everyone sign in, or a tailnet's `tailscale serve`, with UpNext reachable only through it (listening on loopback, or firewalled). Behind Tailscale Funnel, which lets anyone in, leave it off. Its writes still need UpNext's own `Origin`, so the proxy's sign-in cookie can't be ridden from another site.

## Not on the open internet

UpNext is not meant to face the internet directly. To reach it away from home, put it behind something that does authentication properly (Tailscale, a VPN, or a reverse proxy with its own login) and keep the token layer on as well, unless you've listed that proxy and let it do the login.

The token layer stays on behind a proxy even when the proxy rewrites `Host`, because the proxy's forwarding headers, or nginx's default HTTP/1.0, mark its requests as another device's. A proxy that speaks HTTP/1.1, strips every forwarding header and rewrites `Host` to a loopback address can't be told from a browser on this computer, so it would pass its visitors off as this computer. Don't set one up that way.
