# GOG

[API index](../API.md) · [Steam](steam.md) · [More stores](more-stores.md)

GOG playtime comes from GOG's own API after a one-time sign-in. GOG Galaxy syncs its sessions to GOG's servers, so this covers games played on Windows too. With a GOG sign-in, a sync reads the owned games and each game's playtime (source `gog`). Heroic stays the launcher for those games where it's installed; otherwise `run_url` is `goggalaxy://openGameView/gog_{id}`. The `gog` profile source counts as configured with Heroic or a sign-in, with a `note` such as `"Heroic + GOG account"`. The sign-in is kept in the database, on the player's GOG connection.

## GET /api/gog

The `GogStatus`: `{"connected": false, "user_id": null, "username": null, "auth_url": "https://auth.gog.com/auth?client_id=...", "login_pending": false, "login_error": null}`. When connected, `user_id` and `username` are set. `login_pending` is true while a sign-in window is open, and `login_error` says why the last window failed (closed early, no display).

## POST /api/gog/login

`{"code": "<the code from the redirect, or the whole redirect address>"}` answers with the `GogStatus`, connected. `400` `{"error": …}` when the code is wrong or has expired. This is the way to paste GOG's landing-page address by hand.

## DELETE /api/gog

Deletes the stored sign-in and answers with the `GogStatus`.

## The sign-in window

GOG's sign-in only sends the browser back to its own `https://embed.gog.com/on_login_success` page (a localhost `redirect_uri` fails with `redirect_uri_mismatch`), so a browser tab can't hand the code to UpNext. Instead, `POST /api/gog/window` opens a sign-in window on the computer running UpNext (`upnext gog-window`: a wry webview, WebKitGTK on Linux, WebView2 on Windows, WKWebView on macOS). When the window reaches GOG's landing page it reports the code, the server trades it for a sign-in, and the window closes. The UI polls `GET /api/gog` while `login_pending` is true.

It answers with the `GogStatus`. `409`, saying to sign in with the link and paste instead, when a window is open already, the server was built without the app window, or the computer has no desktop to open it on (`environment.display` in [`GET /api/setup`](setup-wizard.md)). A service started without `WAYLAND_DISPLAY` opens it on the session's Wayland socket. Only on this computer.

## The "Send to UpNext" bookmark

Where no window can open (a container, or a browser on another device), the wizard and Settings offer a bookmark, "Send to UpNext", to drag to the bookmarks bar once. It holds UpNext's address and this install's bookmark key.

1. `GET /api/gog/bookmark` gives `{"key": "<64 hex>"}`, made the first time it's asked for and kept with the install's other local values. It needs a `full` token or a paired browser and is never logged. A copy without keys leaves it out, and a restore or an import keeps this install's, so bookmarks dragged here keep working.
2. `POST /api/gog/browser` gives `{"state": "<32 hex>", "auth_url": "https://auth.gog.com/auth?…", "expires_at": 1759600000}`: a one-time state for the device that asked, good for ten minutes. The page keeps it in `localStorage` and opens `auth_url`. At most 16 wait at once, and a restart forgets them.
3. On GOG's success page, the bookmark sends the tab to UpNext's `/gog/callback#key=…&code=…`. Both are in the fragment, so neither reaches a server log or a proxy, and the page drops it from the address and history.
4. That page sends `POST /api/gog/callback` with `{"state": "…", "code": "…", "key": "…"}` (the code, or GOG's whole address), which answers with the `GogStatus`, connected.

Without this install's key (another site sending the browser to `/gog/callback` with its own code while a sign-in waits, or a bookmark dragged before the key existed) the callback is `403`, logged without the code, and the state keeps waiting for the real one. It's `403` too when the state wasn't issued to this device, was used already or expired, and `400` when GOG turns the code down. The first try that has the key uses the state up, whatever its outcome, so a link made elsewhere can't attach another GOG account to this UpNext. Neither the code, the key nor the state is logged.
