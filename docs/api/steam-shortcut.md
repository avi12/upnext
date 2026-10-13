# Big screen and the Steam shortcut

[API index](../API.md) · [Steam](steam.md) · [Only on this computer](local-only.md)

The app window opens in big screen, full screen, with `upnext app --big-screen` or `upnext-app --big-screen`, which the Start menu's "UpNext (big screen)" entry, the Linux launcher's "Big screen" action and the Steam shortcut run. It does the same when Steam starts it from a Steam Deck's Gaming mode or Big Picture (`SteamGamepadUI=1` or `SteamDeck=1` in its environment). F11 goes in and out of full screen. A window opened this way has no tray icon, so closing it ends it, and the server inside it when it runs one, as the launcher that started it expects. The page learns it from `window.upnextBigScreen`, which turns big screen on for that window without changing what the browser remembers.

Both routes answer only on this computer, since they read and write Steam's files there.

## GET /api/steam/shortcut

```json
{"target": {"program": "/usr/bin/upnext", "arguments": "app --big-screen"},
 "accounts": [{"account_id": "12345678", "added": false, "layout": false, "layout_error": null, "error": null}],
 "steam_running": false, "unavailable": null}
```

- `target`: what a launcher runs to open UpNext in big screen. On Windows it's `upnext-app.exe` beside the running program with `--big-screen`; elsewhere the running program with `app --big-screen` (`--big-screen` when it's `upnext-app`). Settings shows it, with a copy button, for the launchers UpNext can't add itself to (the Xbox app, Armoury Crate SE, Legion Space). It's null in a container, a build without the app window, or where that program isn't there. In the Flatpak it's `/usr/bin/flatpak` with `run io.github.zytocc.UpNext --big-screen`, and in UpNext's Steam copy the copy's own launcher with `--big-screen` (the Linux depot's `upnext.sh`, or `upnext-app.exe` on Windows); `accounts` is empty in both.
- `accounts`: each Steam account that has signed in on this computer (the numbered folders of Steam's `userdata`), and whether its library has UpNext's shortcut. `layout` says whether the shortcut has a controller layout for every kind of controller Add to Steam writes one for; a layout file UpNext can't read counts as done. A shortcut without them gets them from Add to Steam, which leaves the shortcut as it is. `layout_error`, while `layout` is false, says why the last Add to Steam couldn't write the layouts (until a press works or the server restarts), and Settings then shows how to pick the layout in Steam instead. `error` says why the account's file can't be read.
- `steam_running`: adding waits until Steam is closed.
- `unavailable`: why Add to Steam can't be used here (a container, no app window or program for it, no Steam or no account, the Flatpak, whose sandbox sees Steam's folders read-only, or the Steam copy, which Steam lists already), or null.

## POST /api/steam/shortcut

Adds UpNext as a non-Steam shortcut to every account's `userdata/<account>/config/shortcuts.vdf`: named UpNext, running `target`, with UpNext's library images in `config/grid` under the id Steam derives from the program and the name (images there already, such as artwork chosen in Steam, are left alone).

- Every account's file is read before any is written, so one that can't be read changes none. Before UpNext first changes a file it copies it to `shortcuts.vdf.upnext-backup`, and keeps that first copy. The other shortcuts stay byte for byte as they were, and an account whose library has UpNext already is left alone, so pressing it twice changes nothing.
- It gives the shortcut Steam's Gamepad controller layout, so a pad works as a controller at once rather than moving the mouse. In each account's `steamapps/common/Steam Controller Configs/<account>/config/configset_controller_<type>.vdf`, for `xbox360`, `xboxone`, `ps3`, `ps4`, `ps5`, `switch_pro`, `neptune` (a Steam Deck) and `generic`, it adds `"upnext" { "template" "controller_<type>_gamepad_joystick.vdf" }`, as Steam ROM Manager writes them. A file with a layout for UpNext already is left as it is; another is copied to `….vdf.upnext-backup` the first time, keeping every other entry. A file it can't read is logged and left alone, and the shortcut is added all the same.
- Steam Cloud can replace these files. When the pad still moves the mouse, pick the layout by hand: UpNext's page in Steam, the controller icon, the layout, Templates, Gamepad.

Answers `200` in the shape above, or `409` with why when Steam is running ("Close Steam first"), an account's file can't be read, or it can't be used here.
