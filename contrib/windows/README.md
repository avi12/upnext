# Windows

What the Windows builds are made from:

- `upnext.ico`, `upnext.manifest`: the icon and the application manifest that `build.rs` puts into both exes on a Windows build, with a version resource. The manifest asks for no administrator rights (`asInvoker`), declares Windows 10 and 11, and makes the window per-monitor DPI aware.
- `upnext.wxs`: the installer, for WiX Toolset 3.14. It installs for the current user only, in `%LOCALAPPDATA%\Programs\UpNext`, so it needs no administrator rights. It adds two Start menu entries, "UpNext" for `upnext-app.exe` and "UpNext (big screen)" for `upnext-app.exe --big-screen`, and puts the folder on the user's `PATH`, so `upnext` works in terminals opened afterwards. Uninstalling removes those, and the `UpNext` value that Start with Windows adds under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (with `reg.exe`, through WiX's util extension, since the app writes that value and the installer doesn't own it; an upgrade keeps it), and nothing else: the library in `%LOCALAPPDATA%\upnext` stays.
- `upnext.wxl`, `installer-notice.rtf`, `installer-banner.bmp`, `installer-dialog.bmp`: the installer's wording and pictures. The notice is the page shown before installing, in place of a licence agreement. The pictures are 8-bit BMPs drawn from the 256 px icon, at WixUI's sizes (493 × 312 and 493 × 58).
- `build-msi.ps1`: builds `upnext-x86_64-pc-windows-msvc.msi` and its `.sha256` from the two exes.
- `msix/AppxManifest.xml`, `msix/Assets/`: the Microsoft Store package's manifest and its pictures, drawn from the same icon at the sizes and scales Windows asks for (`.scale-`, `.targetsize-` and `_altform-unplated` in their names). The manifest's comments say what each part is for.
- `build-msix.ps1`: builds `upnext-x86_64-pc-windows-msvc.msix` and its `.sha256` from the two exes, and leaves the unpacked package in `layout\` beside them. It puts `msix-package.txt` beside the programs, which tells UpNext it runs from the package (`src/msix.rs`).

The release workflow's `build-msi` job builds the MSI on a `v*` tag from the exes in dist's Windows zip, and its `build-msix` job the Store package, once Partner Center's identity is set (`docs/dev/RELEASE.md`, "The Microsoft Store package"). Between releases, build and check them on a Windows machine, as below; the `windows` workflow, run by hand, builds the Store package too.

## Building an MSI on Windows

What the machine needs, once:

- Git and Node.js 22.18 or newer.
- Rust stable with the MSVC toolchain (`rustup`, default host `x86_64-pc-windows-msvc`), and Visual Studio 2022 Build Tools with "Desktop development with C++", which brings the linker and the Windows SDK's `rc.exe` that `build.rs` uses. `winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`, then `winget install --id Rustlang.Rustup`.
- Nothing for WiX: `build-msi.ps1` downloads WiX 3.14.1's binaries zip from WiX's GitHub release into `target\wix-3.14.1` the first time, checks its SHA-256 and unpacks it. It runs on the .NET Framework 4.8 that Windows includes. To use a WiX 3.14 that's already installed, pass `-WixDir "$env:WIX\bin"`.

In PowerShell, in the checkout:

```powershell
cd web; npm ci; npm run build; cd ..
cargo build --release --locked --bins
powershell -ExecutionPolicy Bypass -File contrib\windows\build-msi.ps1
```

`-ExecutionPolicy Bypass` runs this one script without changing the machine's policy. The MSI is `target\wix\upnext-x86_64-pc-windows-msvc.msi`, with Cargo.toml's version. `-Version 0.1.1` builds one with a higher version, to test an upgrade.

## Checking an MSI

Start UpNext from the Start menu or Explorer for these checks, not from a terminal inside another app: an app installed as a package (MSIX) can run what its terminal starts with its own virtualised copies of the registry and AppData, so the Run key, the settings and the library would be written there and not where Windows and UpNext look for them.

1. Close UpNext. Double-click the MSI. SmartScreen may say "Windows protected your PC" (More info, Run anyway), since the build isn't signed. The installer's one page says where UpNext goes and that uninstalling keeps the library; Install shows no administrator prompt.
2. `%LOCALAPPDATA%\Programs\UpNext` holds `upnext.exe`, `upnext-app.exe`, `LICENSE-MIT`, `LICENSE-APACHE` and `THIRD_PARTY_NOTICES.txt`, and nothing else.
3. Start, "UpNext": the app opens on your library, with UpNext's icon in the Start menu, the title bar and the taskbar. Start has one other UpNext entry, "UpNext (big screen)", which opens it full screen in big screen (F11 leaves full screen, and closing it ends it); the desktop has no shortcut.
4. In a new terminal, `upnext --version` prints the version with the commit, and `(Get-Command upnext).Source` is the installed `upnext.exe`. `[Environment]::GetEnvironmentVariable('Path', 'User')` ends with `...\Programs\UpNext\`.
5. Settings, Apps, Installed apps lists "UpNext" by "UpNext contributors" with the icon.
6. Build again (another commit, or the same one) and install that MSI over it: Installed apps still lists one UpNext, and the library is as it was. An MSI built with a lower `-Version` than the one installed refuses to install.
7. Uninstall from Installed apps: the folder, both Start menu entries and the `PATH` entry go (check in a new terminal), and `%LOCALAPPDATA%\upnext` is untouched. Installing again picks up the same library. Before uninstalling, check that Windows Installer recorded where it's installed: `Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\*\Products\*\InstallProperties" | ? DisplayName -eq UpNext | select InstallLocation` names `...\Programs\UpNext\`.
8. The tray. With the app open, UpNext's icon is by the clock (perhaps under the arrow). Close the window: the icon stays, Task Manager still lists `upnext-app.exe`, and `http://127.0.0.1:8787` still answers in a browser. A left click on the icon opens the window again at its last size and zoom; its menu has Open UpNext, Start with Windows and Quit. Quit ends `upnext-app.exe`. In Settings, Windows, turn off "Keep running in the background": closing the window now ends it. Turn it back on.
9. Start with Windows, from the tray's menu or Settings, Windows: `Get-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Run | select UpNext` shows `"…\Programs\UpNext\upnext-app.exe" --background`, and Task Manager's Startup apps lists it. Sign out of Windows and back in: the icon appears with no window. The menu's checkmark follows a change made in Settings. Turn it off: the value goes. Turn it on again, upgrade with a newer MSI (step 6): the value stays; uninstall: it goes.

## Building the Store package

On the same machine, after the release build above (the Windows SDK it needs comes with the Build Tools' C++ workload):

```powershell
powershell -ExecutionPolicy Bypass -File contrib\windows\build-msix.ps1
```

It writes `target\msix\upnext-x86_64-pc-windows-msvc.msix` and the unpacked `target\msix\layout`. Without `-IdentityName`, `-Publisher` and `-PublisherDisplayName` (Partner Center's values) it uses placeholders, `UpNext.Test` by `CN=UpNext Test`, which the Store won't take but a test can. The package isn't signed, since the Store signs it, so Windows won't install the `.msix` itself outside the Store; a machine in Developer Mode registers the unpacked folder instead. The `windows` workflow's artifact has both.

## Checking the Store package

Uninstall the MSI first if it's installed (the library stays), so Start and the terminal have one UpNext. Turn on Developer Mode (Settings, System, For developers), then in PowerShell, in the folder that holds `layout`:

```powershell
Add-AppxPackage -Register .\layout\AppxManifest.xml
```

1. Start lists "UpNext" and "UpNext (big screen)" with UpNext's icon, and each opens as from the MSI; "UpNext" opens the library that's in `%LOCALAPPDATA%\upnext`.
2. In a new terminal, `upnext --version` prints the version, and `(Get-Command upnext).Source` is under `%LOCALAPPDATA%\Microsoft\WindowsApps`. `upnext-app` opens the usual window, not big screen: its alias belongs to the "UpNext (big screen)" entry, whose `--big-screen` an alias doesn't pass on.
3. Change a setting and close UpNext from the tray's Quit. The files in `%LOCALAPPDATA%\upnext` changed, and `%LOCALAPPDATA%\Packages\UpNext.Test_*\LocalCache\Local` has no `upnext` folder: the package writes the real library, not a copy of its own.
4. Settings, Windows, Start with Windows: turning it on says to use Windows Settings, Apps, Startup, which lists UpNext. Turn it on there and sign out and back in: the tray icon appears with no window.
5. With Steam closed, Settings, Big screen, Add to Steam; the shortcut's target is `%LOCALAPPDATA%\Microsoft\WindowsApps\upnext-app.exe --big-screen`, and starting it from Steam opens big screen.
6. With Claude Code or Codex chosen in Settings, Picks, a pick works as from the MSI, and its sign-in is the one the terminal uses.
7. `Get-AppxPackage UpNext.Test | Remove-AppxPackage` removes it; `%LOCALAPPDATA%\upnext` stays.

The release build's file submission to Microsoft, the Store submission, and checking a published release are in `docs/dev/RELEASE.md`.
