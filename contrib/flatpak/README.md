# UpNext's Flatpak

`io.github.zytocc.UpNext`, built from this repository's source with no network, on GNOME's runtime 51. The app window is the Flatpak's command; the command line is `flatpak run --command=upnext io.github.zytocc.UpNext`. [Installing the Flatpak](../../docs/install/flatpak.md) is for people installing it; this file is for building it and for the Flathub submission (`docs/dev/RELEASE.md`, "The Flatpak").

| File | What it is |
| --- | --- |
| `io.github.zytocc.UpNext.yml` | The manifest. Its source is this checkout as it is on disk (`type: dir`), less what builds leave in it. Flathub's copy takes the release tag instead. |
| `io.github.zytocc.UpNext.desktop` | The launcher entry, with the "Big screen" action. |
| `io.github.zytocc.UpNext.metainfo.xml` | What software centres such as Discover show: the description, screenshots, releases, and the controls UpNext supports. |
| `generate-sources.sh` | Writes `generated/`: `cargo-sources.json` (every crate in `Cargo.lock`) and `node-sources.json` (every npm package the web build installs, less Playwright), with flatpak-builder-tools' generators at a pinned commit, and `commit`, the checkout's commit. `build.sh` runs it, so it follows the lock files. |
| `build.sh` | Writes the sources, builds with flatpak-builder into a repository and a bundle under `target/flatpak/`, and with `--lint` runs Flathub's linter. |
| `smoke-test.sh` | Installs a bundle for the current user, runs `upnext --version` and `upnext serve` on a made-up library inside the sandbox, checks it names the commit in `generated/commit` (when that file is there) and knows it's in a Flatpak, and removes it again. |
| `lint-exceptions.json` | The linter's expected findings: the read-only access to the Steam and Heroic Flatpaks' folders, which the reviewers have to grant (below), and the screenshots, which Flathub's build copies to its media server. |

## Building it

You need Flatpak, flatpak-builder (or `org.flatpak.Builder` from Flathub), git, Python 3 with `venv`, npm, and Flathub set up as a remote for your user. Then, from the repository's root:

```sh
contrib/flatpak/build.sh                  # target/flatpak/upnext-<version>-x86_64.flatpak
contrib/flatpak/build.sh --lint           # and Flathub's linter
flatpak install --user target/flatpak/upnext-*.flatpak
flatpak run io.github.zytocc.UpNext
```

The first build downloads the GNOME 51 runtime and SDK and the Rust and Node SDK extensions (a few GB) and compiles every crate; later builds reuse what they can.

The build copies the source without `.git`, so the manifest hands `generated/commit` to the build as `UPNEXT_COMMIT` (`build.rs`). `flatpak run --command=upnext io.github.zytocc.UpNext --version` then shows it, as in `0.1.0 (abc1234)`, and `/api/health` has it as `build`. `build.sh` rewrites the file on every build and the smoke test checks the bundle against it, so a bundle built from older sources is caught. Flathub's copy, built from a tag, sets `UPNEXT_COMMIT` to the tag's commit in the module's `env` instead.

`contrib/flatpak/smoke-test.sh BUNDLE` tries a bundle without touching a real install: it refuses to run when `io.github.zytocc.UpNext` is already installed for your user.

## Permissions

What the manifest asks for, and why, for Flathub's reviewers:

- `--share=network`: the stores' APIs, the AI, the other devices that use UpNext (phones, the Android app, another computer's window), and the mDNS announcement they find it by.
- `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`: the window.
- `--device=all`: the GPU for WebKitGTK, and gamepads for big screen, which WebKitGTK reads through libmanette from `/dev/input`. `--device=input` is narrower but known only to Flatpak 1.15.6 and later; Ubuntu 24.04 and Debian 12 have 1.14, where a pad would never show up.
- `--filesystem=~/.local/share/Steam:ro`, `~/.steam:ro`, `~/.var/app/com.valvesoftware.Steam/.local/share/Steam:ro`: Steam's own files, natively (Debian's package keeps them in `~/.steam/debian-installation`) and as a Flatpak. UpNext reads the games owned, their playtime and the signed-in account from them (`src/steam_local.rs`). Read-only: in the Flatpak, "Add to Steam" gives steps instead of writing a shortcut.
- `--filesystem=~/.config/heroic:ro`, `~/.var/app/com.heroicgameslauncher.hgl/config/heroic:ro`: Heroic's library caches for Epic, GOG and Amazon (`src/heroic.rs`), natively and as a Flatpak. Read-only.

Reading these launchers' libraries is what the app is for, and no portal hands one program another's folders. If the reviewers refuse the static access, the fallback is choosing each folder once through the file chooser portal.

Nothing else is needed:

- **Playing** opens `steam://`, `heroic://` and `goggalaxy://` links with the runtime's `xdg-open`, through the OpenURI portal, which may ask once which program opens them.
- **Gamepad layouts**: the window adds a layout for the Xbox controller on Microsoft's wireless adapter to `~/.var/app/io.github.zytocc.UpNext/config/libmanette/gamecontrollerdb`, where the sandbox's libmanette reads it.
- **Closing when a game starts under Steam**: Steam's `SteamGamepadUI`, `SteamTenfoot` and `SteamDeck` variables pass through `flatpak run`, so a window Steam starts from Big Picture or Gaming mode opens in big screen and closes after Play as natively.
- **mDNS**: the sandbox shares the host's network, so the announcement on UDP 5353 works as natively.
- **Claude Code** goes into the app's own folder (`upnext install-claude`), which UpNext runs it with as its home, so it stays inside the sandbox. It signs in with a token from `claude setup-token`, set with `flatpak override --env`. The Codex CLI isn't available.
- **No service and no start at login**: UpNext runs while its window is open.
- **Certificates**: the runtime brings its own. An `SSL_CERT_FILE` or `SSL_CERT_DIR` set on the host can name a file the sandbox doesn't show; UpNext then restarts itself without it, uses the runtime's certificates, and logs the `flatpak override --user --filesystem=<its folder>:ro io.github.zytocc.UpNext` line that shares it (`src/flatpak.rs`).
- **Another UpNext on the same port**: a Flatpak window opened while a native UpNext serves on 8787 opens on that one, with its library, as two native windows do.
