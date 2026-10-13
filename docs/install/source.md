# Building from source

You need [Rust](https://rustup.rs) 1.90 or newer and Node 22.18 or newer with npm. On Linux, the app window and the GOG login window also need WebKitGTK 4.1 and GTK 3 (`sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config` on Debian and Ubuntu, `sudo pacman -S webkit2gtk-4.1 gtk3` on Arch).

```sh
git clone https://github.com/zytocc/upnext && cd upnext
npm ci --prefix web                       # the web UI: required, since the binary embeds it
npm run build --prefix web
cargo install --path . --locked           # installs ~/.cargo/bin/upnext
upnext serve                              # then open http://127.0.0.1:8787
```

`cargo install --path . --locked --no-default-features` builds the server alone, without GTK or WebKitGTK: no app window, and GOG sign-in by pasting a link instead of a login window.

On Windows, you need Rust with the MSVC toolchain and Visual Studio 2022 Build Tools ("Desktop development with C++") in place of GTK; each line above works as written in PowerShell.

[Running UpNext](../running.md) says how to start it at login, and [CONTRIBUTING.md](../../CONTRIBUTING.md) how to work on it.
