# Linux

## Debian, Ubuntu and Fedora

Each [release](https://github.com/zytocc/upnext/releases) has packages for x86_64 and arm64: a `.deb` for Debian 12 and 13 and Ubuntu 22.04, 24.04 and 26.04, and an `.rpm` for Fedora 43 to 45. They put `upnext` and `upnext-app` in `/usr/bin`, UpNext in your launcher under Games, and the user service in place, and your package manager brings WebKitGTK and GTK with them. Set `V` to the release's version; on arm64 the files end in `_arm64.deb` and `.aarch64.rpm`.

```sh
V=0.1.0
# Debian and Ubuntu
curl -LO "https://github.com/zytocc/upnext/releases/download/v$V/upnext_$V-1_amd64.deb"
curl -LO "https://github.com/zytocc/upnext/releases/download/v$V/upnext_$V-1_amd64.deb.sha256"
sha256sum -c "upnext_$V-1_amd64.deb.sha256"
sudo apt install "./upnext_$V-1_amd64.deb"
# Fedora
curl -LO "https://github.com/zytocc/upnext/releases/download/v$V/upnext-$V-1.x86_64.rpm"
curl -LO "https://github.com/zytocc/upnext/releases/download/v$V/upnext-$V-1.x86_64.rpm.sha256"
sha256sum -c "upnext-$V-1.x86_64.rpm.sha256"
sudo dnf install "./upnext-$V-1.x86_64.rpm"
# openSUSE Tumbleweed: the same .rpm, which isn't signed
sudo zypper install --allow-unsigned-rpm "./upnext-$V-1.x86_64.rpm"
```

The `.rpm` asks for WebKitGTK by its library so that openSUSE can install it too. Since that change it hasn't been installed on Fedora or on openSUSE Tumbleweed; the first release's checks install it on Fedora. If it gives you trouble, [open an issue](https://github.com/zytocc/upnext/issues).

Then open UpNext from your launcher, or run `upnext app` for the window or `upnext serve` for the web UI. Nothing starts it at login until you run `systemctl --user enable --now upnext`. If you ran `upnext install-desktop` before, run `upnext install-desktop --remove`, or your own launcher entry hides the package's.

Removing the package (`sudo apt remove upnext`, `sudo dnf remove upnext` or `sudo zypper remove upnext`) leaves your library in `~/.local/share/upnext`. Run `systemctl --user disable --now upnext` first if you turned the service on.

## Arch Linux

Each release has a `PKGBUILD` for `upnext-bin`, which installs that release's Linux archive, checked against its SHA-256, as a pacman package:

```sh
mkdir upnext-bin && cd upnext-bin
curl -fsSLO https://github.com/zytocc/upnext/releases/latest/download/PKGBUILD
less PKGBUILD        # read it first, as with any PKGBUILD
makepkg -si          # downloads the release archive, checks it, builds and installs the package
```

The same `PKGBUILD` builds on Arch Linux ARM (aarch64). Or download `upnext-bin-<version>-1-x86_64.pkg.tar.zst` from the release and run `sudo pacman -U ./upnext-bin-<version>-1-x86_64.pkg.tar.zst`. Give pacman the downloaded file rather than the link: pacman asks for a signature on a package it fetches itself, and this one isn't signed.

Either way, pacman brings WebKitGTK and GTK and puts `upnext` and `upnext-app` in `/usr/bin`, UpNext in your launcher under Games, and the user service in place. Nothing starts it at login until you run `systemctl --user enable --now upnext`. For a new version, build the new release's `PKGBUILD` the same way. Removing the package (`sudo pacman -R upnext-bin`) leaves your library in `~/.local/share/upnext`.

If you had UpNext from `cargo install` before, run `cargo uninstall upnext`, since `~/.cargo/bin` comes before `/usr/bin` on your `PATH`. If you copied the unit to `~/.config/systemd/user/upnext.service`, remove that copy, which overrides the package's, and run `systemctl --user daemon-reload`. If you ran `upnext install-desktop`, run `upnext install-desktop --remove`.

## Release archive

Each release also has an archive for Linux on x86_64 and arm64, with `upnext` and `upnext-app` and the web UI built in. It needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36, Arch and later), and WebKitGTK 4.1 and GTK 3 for the app window, which most desktops already have (`sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0` on Debian and Ubuntu, `sudo pacman -S webkit2gtk-4.1 gtk3` on Arch, `sudo dnf install webkit2gtk4.1 gtk3` on Fedora).

```sh
curl -LO https://github.com/zytocc/upnext/releases/latest/download/upnext-x86_64-unknown-linux-gnu.tar.xz
curl -LO https://github.com/zytocc/upnext/releases/latest/download/upnext-x86_64-unknown-linux-gnu.tar.xz.sha256
sha256sum -c upnext-x86_64-unknown-linux-gnu.tar.xz.sha256
tar xf upnext-x86_64-unknown-linux-gnu.tar.xz
install -Dm755 -t ~/.local/bin upnext-x86_64-unknown-linux-gnu/upnext upnext-x86_64-unknown-linux-gnu/upnext-app
upnext serve                               # then open http://127.0.0.1:8787
```

On arm64, the archive is `upnext-aarch64-unknown-linux-gnu.tar.xz`. `~/.local/bin` has to be on your `PATH`; on most distributions it is once it exists and you log in again. [Running UpNext](../running.md#start-it-at-login-linux) says how to start it at login.

## Other ways

- [Flatpak](flatpak.md), for any Linux with Flatpak and for a Steam Deck.
- [Building from source](source.md).

Next: [Stores and accounts](../stores.md).
