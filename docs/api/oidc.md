# Signing in with an identity provider

[API index](../API.md) · [Players](players.md) · [Who can use the API](access.md)

With `oidc_issuer`, `oidc_client_id` and `public_url` set, the page another device lands on offers "Sign in with <provider>". It pairs a browser and finds or makes its player. UpNext is the relying party (the authorization code flow with PKCE, the provider found by discovery from its issuer), and players stay in the local database. The three `GET` routes below need no token, but not over plain HTTP from a public address (`403`).

Set it up at the provider as a confidential client (or a public one, with no secret) whose redirect address is `public_url` followed by `/api/v1/oidc/callback`, with the `openid`, `profile` and `email` scopes, and `groups` when `oidc_allowed_groups` is used.

## GET /api/oidc

`{"enabled": true, "name": "Authelia"}`: whether the sign-in is offered, and the button's name (`oidc_name`, else the issuer's host).

## GET /api/oidc/login

`303` to the provider's sign-in page, with a state, a nonce and a PKCE challenge. The provider's discovery answer is kept for five minutes. The state waits ten minutes, tied to this browser by an `upnext_oidc_signin` cookie (`SameSite=Lax`, `Path=/api/v1/oidc/callback`). Three wait at most from one address (an IPv6 /64), a fourth pushing out that address's oldest; 32 wait at most in all, and then a new one is refused (`/#signin_error=`), never pushing out another device's. The way back ends only the sign-in its own state names. When the sign-in isn't set up or the provider can't be reached, it goes to `/#signin_error=<why>`.

## GET /api/oidc/callback?state=…&code=…

The provider's way back. The code is traded for the ID token (`client_secret_basic`, or `client_secret_post` when that's all the provider takes; the client id alone without a secret), and the token is checked: signed with one of the provider's published keys (RS256, RS384, RS512, PS256, PS384, PS512, ES256, ES384 or EdDSA; never `none` or one signed with the secret), `iss` the provider's own, `aud` holding the client id (and `azp`, when given, the client id), not expired and not before its `iat` or `nbf` (two minutes' leeway), and the nonce this sign-in was given.

The account (`iss` and `sub`) is then its linked player. While "Link the next sign-in" waits for a player, an account no player has isn't linked or made a player: it waits to be confirmed on this computer, and the browser goes to `/#signin_waiting=<what next>` with no token. Otherwise, with `oidc_auto_provision` on (the default), it gets a new player named from `name`, else `preferred_username`, else "Player N", with ` 2`, ` 3`… when the name is taken. Players are never matched by name or email. With `oidc_auto_provision` off, an unknown account is refused, saying a player must be linked first. With `oidc_allowed_groups` set (comma-separated), the token's `groups` claim must hold one of them.

The browser gets a device token bound to the player, of `full` scope and `kind: "oidc"`, named from the browser and the provider ("Android phone (Authelia)"), in the `upnext_device_<port>` cookie, and goes to `/`. A known account gets a new device token for its player. On any failure, `303` to `/#signin_error=<why>` with nothing made. The state works once.

## POST /api/oidc/logout

`204`, revoking this browser's device token and clearing its cookie. It doesn't sign out of the provider. `400` for a browser that didn't sign in this way.

## Linking an account to a player

All of these are only on this computer.

- **`POST /api/players/{id}/oidc/link`** `{"pin": "1234"}` (the PIN when the player has one, else `{}`) → the player, with `oidc_link_until`: "Link the next sign-in". For ten minutes, an account no player is linked to that signs in waits to be confirmed as this player (`oidc_waiting`), in place of any that waited before. One player at a time; arming another replaces it. `400` when no issuer is set, `403` from another device or for a wrong PIN, `429` while locked.
- **`POST /api/players/{id}/oidc/confirm`** `{"subject": "u-1", "pin": "1234"}` → the player, now with `oidc`. The account waiting (named by its `subject`, as `oidc_waiting` showed it, so a newer sign-in can't take its place) is linked to the player and the link ends; the account signs in as them from then on, so the browser signs in again. An account the player had before is unlinked, and the browsers it signed in are signed out. Needs the player's PIN again when they have one. `409` when that account isn't waiting for this player any more (the link ran out, was ended, or a newer sign-in replaced it); `403` and `429` as above.
- **`DELETE /api/players/{id}/oidc/link`** → the player: the link ends, with any account waiting.
- **`DELETE /api/players/{id}/oidc`** → the player: unlinks their account, ends a link waiting for them, and revokes the device tokens that account signed in as them.

A player's `oidc`, `oidc_link_until` and `oidc_waiting` fields show where this stands ([Players](players.md#get-apiplayers)).

## Settings

`oidc_issuer` (`https://`, or `http://` on this computer or its network; kept as typed, since the ID token's `iss` must match it exactly), `oidc_client_id`, `oidc_client_secret` (write-only: `GET /api/settings` gives `oidc_client_secret_set`), `oidc_name`, `oidc_auto_provision` (`true`) and `oidc_allowed_groups`, each also in the environment (`UPNEXT_OIDC_ISSUER` and so on). `GET /api/settings` gives each with `<key>_source`, and `oidc_redirect_uri`, the address to register at the provider (null without `public_url`). They're set only on this computer or in the environment. Back-channel logout, SCIM and LDAP aren't supported.
