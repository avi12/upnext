# Flatpak and Steam Deck

UpNext's Flatpak, `io.github.zytocc.UpNext`, runs on any Linux with Flatpak and on a Steam Deck, whose system can't take packages. A release can carry it as a single file, `upnext-<version>-x86_64.flatpak`, which installs with the GNOME runtime from Flathub (set up Flathub first, as [flathub.org/setup](https://flathub.org/setup) says). You can also build it from this repository ([contrib/flatpak](../../contrib/flatpak/README.md)).

```sh
flatpak install --user ./upnext-<version>-x86_64.flatpak
flatpak run io.github.zytocc.UpNext                           # the app window; it's in your launcher too
flatpak run --command=upnext io.github.zytocc.UpNext doctor   # the command line
```

`flatpak update` brings new versions.

## What the sandbox changes

- **What it reads.** Steam's and Heroic's usual folders, native and Flatpak, read-only, and nothing else of yours. For a library somewhere else, `flatpak override --user --filesystem=/path/to/Steam:ro --env=UPNEXT_STEAM_DIR=/path/to/Steam io.github.zytocc.UpNext` lets it read the folder and points UpNext at it (`UPNEXT_HEROIC_DIR` for Heroic). The setup wizard shows the same lines.
- **Where it keeps things.** In `~/.var/app/io.github.zytocc.UpNext`, with the database in `data/upnext`, apart from a native install's. To move a library between the two, download a backup in one (Settings › Backups, with keys and sign-ins) and import it in the other.
- **It runs while its window is open.** There is no service and nothing [starts it at login](../running.md#start-it-at-login-linux); schedules run while the window is open.
- **The AI.** An API key or your own server work as anywhere. The sandbox can't run a Claude Code installed on the computer, so install it into UpNext's own folder with `flatpak run --command=upnext io.github.zytocc.UpNext install-claude`, and sign it in with a token from `claude setup-token` (run on any computer where you use Claude): `flatpak override --user --env=CLAUDE_CODE_OAUTH_TOKEN=<the token> io.github.zytocc.UpNext`, then start UpNext again. The Codex CLI can't be used from the Flatpak.
- **Add to Steam** can't change Steam's files from the sandbox, so add UpNext in Steam yourself (below).
- **Play** hands `steam://` and `heroic://` links to your desktop through Flatpak's portal, which may ask once which program opens them.
- **Gamepads** in big screen need the sandbox to see `/dev/input`, which the Flatpak asks for.
- **Certificates.** An `SSL_CERT_FILE` or `SSL_CERT_DIR` set on your computer (for a company's proxy, say) passes into the sandbox, which may not show that file. UpNext then uses the runtime's own certificates and says so in its log, with the line that shares the file: `flatpak override --user --filesystem=/path/to/its/folder:ro io.github.zytocc.UpNext`.

## On a Steam Deck

1. In Desktop mode, install the Flatpak from Konsole as above.
2. Open Steam there and choose Games › Add a Non-Steam Game to My Library. Tick UpNext and add it.
3. In its Properties, put ` --big-screen` at the end of the launch options.
4. On its page, choose the controller icon, the layout, Templates, Gamepad. Steam gives a program it didn't install the desktop layout, where the pad moves the mouse.

Back in Gaming mode, UpNext is in your library under Non-Steam and opens in [big screen](../big-screen.md). Flatpaks live outside SteamOS's system image, so a SteamOS update keeps UpNext and your library.

Next: [Stores and accounts](../stores.md).
