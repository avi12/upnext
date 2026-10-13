# Running UpNext

`upnext serve` runs the web UI at `http://127.0.0.1:8787` and the scheduled jobs: by default a daily sync, details for new games after each sync, a monthly refresh of player reviews and a weekly backup. Jobs that use the AI are off until you turn them on (Settings › Jobs).

In a terminal, `upnext serve` prints a line for each request: its method, path, status and how long it took, never the query string or the headers. A service's log keeps to warnings; to see the requests there too, set `RUST_LOG=upnext=warn,upnext::request=info` (for the systemd service, with `systemctl --user edit upnext`, as `Environment=` under `[Service]`).

## Start it at login (Linux)

The Debian, Fedora and Arch packages install a systemd user service; `systemctl --user enable --now upnext` turns it on. Otherwise, install it yourself:

```sh
install -Dm644 contrib/systemd/upnext.service ~/.config/systemd/user/upnext.service
systemctl --user daemon-reload
systemctl --user enable --now upnext
journalctl --user -u upnext                # its log
```

That service runs `~/.cargo/bin/upnext`, where `cargo install` puts it. With a release archive, the file is in the archive's folder; point it at `~/.local/bin` as you install it:

```sh
mkdir -p ~/.config/systemd/user
sed 's|%h/.cargo/bin|%h/.local/bin|' upnext-x86_64-unknown-linux-gnu/upnext.service > ~/.config/systemd/user/upnext.service
```

`systemctl --user stop upnext` stops a running job where it can pick up again next time. `loginctl enable-linger` keeps it running while you're logged out. On Windows, the app's tray icon does this job ([Windows](install/windows.md)).

## The app window

`upnext app` opens UpNext in its own window. When the service (or `upnext serve`) is running, the window shows it; when it isn't, the window runs UpNext itself until it closes (on Windows, until you quit it from its icon by the clock). Links to other sites, the Play buttons and Steam sign-in open in your browser or launcher. The window remembers its size and zoom.

Ctrl+Q quits, Ctrl+R or F5 reloads, Ctrl+plus and Ctrl+minus zoom, and Ctrl+0 resets. On Windows and Linux the window has no system title bar: as in Steam and GOG Galaxy, UpNext's top bar is the title bar, with minimize, maximize and close after Settings. Drag the bar to move the window and double-click it to maximize; the edges resize it.

To put it in your launcher under Games, run `upnext install-desktop` (`--remove` takes it out). The packages do this for you; with a package installed, remove your own entry, or it hides the package's.

On NVIDIA with Wayland, WebKitGTK sometimes shows an empty window. `WEBKIT_DISABLE_DMABUF_RENDERER=1 upnext app` is the usual fix; to keep it, add that before `upnext` in the `Exec` line of `~/.local/share/applications/upnext.desktop`.

## Where it keeps things

On Linux, the database and backups are in `~/.local/share/upnext` (`UPNEXT_DATA_DIR` moves them), `.env` is read from `~/.config/upnext`, and `~/.cache/upnext` is the AI CLI's working directory. On Windows the same three are `data`, `config` and `cache` in `%LOCALAPPDATA%\upnext`. The Flatpak keeps its own, in `~/.var/app/io.github.zytocc.UpNext`. The database holds the GOG and Epic sign-ins too.

Every setting can be changed in Settings or with `upnext config set <key> <value>`. App settings override `.env`, which overrides the defaults; `.env.example` lists the common variables, and `upnext config list` every setting. In a `.env` on Windows, put paths in single quotes (`UPNEXT_DATA_DIR='D:\Games\upnext'`): outside quotes a backslash is read as an escape.

A checkout that ran UpNext from its own `data` folder moves it with `upnext migrate-data --from ./data`.

## Backups

Settings › Backups lists the backups next to the database, makes one with a note, downloads any of them with or without your keys and sign-ins, restores one, and imports a database from another computer. A restore or an import backs up the database it replaces first, and UpNext backs up before every schema upgrade. `upnext export <file>` writes a copy without keys and sign-ins.

## Next

- [Big screen](big-screen.md), for a controller or a TV.
- [Players](players.md), when several people play on one computer.
- [Other devices](other-devices.md): phones, other browsers and scripts.
- [Commands](commands.md): everything `upnext` does in a terminal.
