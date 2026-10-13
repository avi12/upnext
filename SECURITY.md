# Security

## Reporting a vulnerability

Please report security problems privately, through GitHub's private vulnerability reporting: the repository's **Security** tab, **Report a vulnerability**. Don't open a public issue for them. Once a problem is confirmed, a fix and a GitHub security advisory follow, crediting you unless you'd rather not be named.

Only the latest release and `main` get security fixes.

## How UpNext is meant to be used

UpNext is an app for one person or one household. It runs on your own computer or home server and keeps everything in a local SQLite database. It has no accounts and no login page of its own: a household's players are a convenience, not accounts, and signing in with your own identity provider, when you set one up, only pairs a browser. What protects it is where it listens and who it answers.

## Out of the box: this computer only

- The server listens on `127.0.0.1:8787` only. Nothing else on your network can connect.
- A request from this computer needs nothing. That means its peer address is loopback, it names a loopback host (`localhost`, `127.0.0.1`, `[::1]`), it carries none of a proxy's headers (`Forwarded`, `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto`, `X-Real-IP`, `Via`) and it speaks HTTP/1.1 or newer. That's the web UI and the app window. A proxy on the same computer connects from loopback too; its headers, or the HTTP/1.0 nginx speaks to its upstream by default, make its visitors need a token even when it rewrites `Host`.
- **DNS rebinding.** An API request must be addressed to a loopback name or to the address set as `public_url`. A page on `evil.example` that resolves its name to `127.0.0.1` still sends `Host: evil.example`, and gets `403`.
- **Cross-site writes.** A `POST`, `PUT` or `DELETE` must come from one of UpNext's own pages: its `Origin` must be the same host and port it's addressed to (or `public_url`), and a request the browser marks `Sec-Fetch-Site: cross-site` is refused. Another website open in the same browser can't change your settings or start jobs.
- **Cross-site reads.** Any API request the browser marks `Sec-Fetch-Site: cross-site` or `same-site`, or whose `Origin` isn't UpNext's own, a `GET` included, is refused, so another site's image or link can't make the server act. The exceptions are the ways back from Steam's sign-in and an identity provider's, which check their own one-time state.
- **Framing.** Every answer carries `X-Frame-Options: DENY` and `Content-Security-Policy: frame-ancestors 'none'`, so another site can't show UpNext in a hidden frame and steer your clicks, and `X-Content-Type-Options: nosniff` and `Referrer-Policy: same-origin`.

The UI's own files (HTML, scripts, fonts) are served to anyone who can reach the port, since a new device has to load the pairing page. They hold nothing private.

## More

- [Who can reach UpNext](docs/security/devices.md): tokens, pairing, phones and other computers, and what only this computer can do.
- [Sign-in and proxies](docs/security/sign-in.md): an identity provider, trusted proxies, and keeping UpNext off the open internet.
- [Players, PINs and your data](docs/security/data.md): what players keep apart, where keys are kept, and what a backup leaves out.

## What isn't covered

- Anyone with access to your user account or your files. UpNext's files are as safe as your home directory.
- A token you've given away. Revoke it in Settings › Devices and API. Restoring or importing a backup keeps the tokens in use, so a revoked token stays revoked.
- The third-party services UpNext talks to ([Third-party services](docs/THIRD_PARTY.md)).
