# Windows

Each [release](https://github.com/zytocc/upnext/releases) has an installer for Windows 10 and 11 on x64, `upnext-x86_64-pc-windows-msvc.msi`. It installs UpNext for you alone, in `%LOCALAPPDATA%\Programs\UpNext`, without asking for administrator rights. It adds two Start menu entries, "UpNext", which opens the app window, and "UpNext (big screen)", which opens it full screen for a controller ([Big screen](../big-screen.md)). It also puts `upnext` on your `PATH` for terminals opened afterwards.

A newer installer upgrades it in place. Uninstall it in Settings › Apps; your library in `%LOCALAPPDATA%\upnext` stays, and installing again picks it up. The zip beside the installer has the same two programs, `upnext.exe` and `upnext-app.exe`, to run without installing.

## SmartScreen and checksums

UpNext's Windows builds aren't code-signed ([Code signing policy](../../README.md#code-signing-policy)), so Windows is wary of them at first:

- SmartScreen says "Windows protected your PC" when you open the installer. Choose **More info**, then **Run anyway**.
- On a PC with Smart App Control turned on, Windows may block the installer or the programs outright, with no way to run them anyway.
- To check that the file is the one the release published, compare its SHA-256 with the `.sha256` file beside it on the release page. In PowerShell, `(Get-FileHash .\upnext-x86_64-pc-windows-msvc.msi).Hash.ToLower()` must print the first word of that file.

## The icon by the clock

While the app runs, UpNext's icon sits by the clock (Windows may tuck it under the arrow). Closing the window leaves UpNext running there, so the daily sync and other jobs keep their schedules. Click the icon to open the window again, or choose Quit in its menu to stop UpNext. To make closing the window stop it, turn off "Keep running in the background" in Settings › Windows.

"Start with Windows", in the same place and in the icon's menu, starts UpNext when you sign in, with only the icon. It's off until you turn it on, and uninstalling turns it off.

## WebView2

The app window needs Microsoft's WebView2 Runtime, which Windows 11 includes. If it's missing, UpNext says so and offers Microsoft's download page.

## From source

You need Rust with the MSVC toolchain, Visual Studio 2022 Build Tools ("Desktop development with C++"), and Node.js. The steps are those in [Building from source](source.md), with no GTK, and each line works as written in PowerShell. [contrib/windows](../../contrib/windows/README.md) says how to build the installer.

Next: [Stores and accounts](../stores.md).
