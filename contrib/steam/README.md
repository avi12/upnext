# UpNext's Steam copy

UpNext on Steam is free, listed under Player Tools, and uses no Steamworks SDK. It is the release's own programs laid out as two SteamPipe depots, one for Windows and one for Linux and SteamOS. This folder has what makes and uploads them, the store text, and the library images "Add to Steam" gives a non-Steam shortcut. The steps only the maintainer can do in Steamworks are in `docs/dev/RELEASE.md`, "The Steam copy".

| File | What it is |
|---|---|
| `build-depots.sh` | Both depots from a release: fetches the tag's x86_64 Linux archive and Windows zip (or takes them from a folder), checks each against its `.sha256`, and lays out `linux/` and `windows/`. |
| `bundle-linux.py` | The Linux depot: the programs with WebKitGTK and every library they need but those in `linux-exclude.txt`, the launcher, the licences and the third-party notices. |
| `fetch-sources.py` | The Ubuntu source packages of every library in the Linux depot, at exactly the versions it carries, as one tar for the release (below). |
| `linux-exclude.txt` | The libraries the Linux depot leaves to the computer, and why. |
| `upnext.sh` | The Linux depot's launcher, which Steam runs. |
| `smoke-test.sh` | Runs the Linux depot in a container that has no GTK, WebKitGTK or GLib. |
| `upload.sh` | Fills in `build/` with the ids from Steamworks and runs steamcmd; `--dry-run` only shows what it would do. |
| `build/` | SteamPipe's app build and the two depot builds, with `@…@` for the ids. |
| `listing/` | Drafts of the store page's text and the content survey's answers. |
| `library/` | The library images (capsule, header, hero, logo) "Add to Steam" writes for a non-Steam shortcut, at the sizes Steam's library uses. |

## The depots

**Windows:** `upnext.exe`, `upnext-app.exe`, the licences, the third-party notices (`THIRD_PARTY_NOTICES.txt`) and README from the release's zip, and `steam-copy.txt`. Steam runs `upnext-app.exe`, with `--big-screen` for the launch option "UpNext". The app window needs Microsoft's WebView2 Runtime, as the MSI install does.

**Linux and SteamOS:**

```
upnext.sh                     the launcher Steam runs
LICENSE-MIT, LICENSE-APACHE   UpNext's licences, and the README and CHANGELOG, from the release's archive
THIRD_PARTY_NOTICES.txt       the licences of the crates and npm packages built into the programs
bin/upnext, bin/upnext-app    the release's programs
bin/steam-copy.txt            tells UpNext it is the Steam copy
lib/                          WebKitGTK, GTK, GLib, GStreamer's core and the rest, with their RPATH set to this folder
lib/x86_64-linux-gnu/webkit2gtk-4.1/   WebKitGTK's web, network and GPU processes and its injected bundle
lib/gio/modules/              TLS (for GOG's sign-in window) and dconf
lib/gdk-pixbuf/               the SVG image loader and its list
lib/gstreamer-1.0/            empty on purpose (its README.txt says why)
share/glib-2.0/schemas/       GTK's settings schemas, which its file chooser needs
share/licenses/               each bundled package's licence, the common licences they name, and PACKAGES.txt with their versions and where their source is
```

[How the Linux depot works](linux-depot.md) explains where its libraries come from, what it leaves to the computer, and the two changes made to them.

## The libraries' source

Most of the bundled libraries are under the LGPL or the GPL, which ask whoever ships them to make their source available. Each release with a Steam build carries the Ubuntu source packages of every bundled library as `upnext-steam-linux-sources.tar`, made by `fetch-sources.py`; [The libraries' source](sources.md) says how it's made and checked.

## Trying it

```sh
cargo build --release --locked --bins            # with web/dist built first
contrib/steam/bundle-linux.py --bin-dir target/release --out target/steam-linux
contrib/steam/smoke-test.sh target/steam-linux ubuntu:24.04   # the glibc of the machine it was built on, or newer
```

The smoke test needs Docker and Xvfb (and ImageMagick to check the window drew). In a fresh home folder it runs `upnext --version`, a first `upnext serve`, which makes `~/.local/share/upnext/upnext.db`, then `upnext.sh --big-screen` as Steam would; it checks that the window's server answers, that "Add to Steam" says this is the Steam copy, that WebKitGTK's processes run from the depot, and lists the libraries each process loaded from outside it.

`upload.sh --dry-run` with made-up ids shows the filled-in build files without steamcmd:

```sh
STEAM_APP_ID=1000 STEAM_DEPOT_LINUX=1001 STEAM_DEPOT_WINDOWS=1002 contrib/steam/upload.sh --depots target/steam --dry-run
```
