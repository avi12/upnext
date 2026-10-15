# Third-party services: what the project uses

[Third-party services](../THIRD_PARTY.md) › What the project uses. Also: [stores](stores.md), [game data](data.md), [AI](ai.md), [publishing](publishing.md).

These aren't services the app talks to. They host what the project publishes, or are built into what it ships.

## Hosting and stores

- **GitHub** hosts the source, the releases, the container image (`ghcr.io`) and CI.
- **Cloudsmith** hosts the apt and dnf package repositories, under its [open-source hosting policy](https://docs.cloudsmith.com/open-source-hosting-policy): free for a public repository with an SPDX licence, in return for a credit that links back to Cloudsmith wherever the repositories are offered. The release workflow pushes the packages with a service account's API key, and installs fetch packages and metadata from `dl.cloudsmith.io`, as with any distribution mirror. UpNext itself never contacts it.
- **F-Droid**: the recipe in `contrib/fdroid/` has F-Droid build the Android app from the tagged source. There's no fee and no account terms beyond its [inclusion policy](https://f-droid.org/docs/Inclusion_Policy/), which asks for free software, no proprietary libraries (the app uses nothing from Google Play services) and declared anti-features (it has none). Google's developer verification, which certified Android devices enforce for apps installed outside Play too, applies to the APK either way.
- **Flathub**: the manifest in `contrib/flatpak/` has Flathub build the Flatpak from the tagged source. There's no fee; its [requirements](https://docs.flathub.org/docs/for-app-authors/requirements) ask for a build from source with no network, minimal static permissions and valid AppStream metadata, and the maintainers accept its terms when submitting. The build uses GNOME's runtime and SDK and the freedesktop SDK's Rust and Node extensions from Flathub, and flatpak-builder-tools' generators (MIT) at a pinned commit to list the crates and npm packages. None of them is in the repository, and the app never contacts Flathub.
- **Steam**: the build files in `contrib/steam/` upload UpNext's Steam copy, free and listed under Player Tools, through Steamworks' SteamPipe with `steamcmd`. The project's Steamworks account takes the [Steam Distribution Agreement](https://partner.steamgames.com/doc/gettingstarted) and its $100 Steam Direct fee. The store page follows Valve's [content rules](https://partner.steamgames.com/doc/gettingstarted/contentsurvey), with the content survey's AI disclosure (`contrib/steam/listing/content-survey.md`). Nothing of the Steamworks SDK is used or carried, so the copy is the release's own programs. steamcmd runs only where the upload happens; the app never contacts Steamworks.

## The Steam copy's Linux libraries

The Linux depot carries WebKitGTK, GTK, GLib, GStreamer's core and the libraries they need, copied from the Ubuntu 22.04 packages the release's programs are built against ([the Steam build](../../contrib/steam/README.md)). They're unchanged apart from each file's RPATH and the helper programs' folder inside `libwebkit2gtk-4.1`.

Their licences are the LGPL (WebKitGTK, GTK, GLib, GStreamer, libsoup, cairo, pango and others), licences that offer the LGPL or the GPL (GnuTLS's nettle and GMP, elfutils' libdw), the GCC runtime exception (libatomic), and permissive licences. The depot keeps:

- each package's licence in `share/licenses/<package>/copyright`;
- the full texts of the LGPL, the GPL and the other common licences, which those files name only by their path on an Ubuntu system, in `share/licenses/common-licenses/`;
- every package with its version in `share/licenses/PACKAGES.txt`.

Every release with a Steam build carries their source, the Ubuntu source packages of exactly those versions, as `upnext-steam-linux-sources.tar`, which `PACKAGES.txt` names with its address. Ubuntu's archive and Launchpad keep each version too. They stay separate shared libraries that can be swapped for others, as the LGPL asks. None needs a credit in Settings › About. Both depots also carry UpNext's own licences and `THIRD_PARTY_NOTICES.txt` in their top folder.

## The Android app's libraries

The libraries built into the APK are in `android/gradle/libs.versions.toml`. All are under the Apache License 2.0, which asks that a copy of the licence and any NOTICE go with the app. The APK carries the list below with the licence's text (`android/app/src/main/assets/third_party_notices.txt`, generated with the main notices), shown by "Open-source licences" on the app's first screen. None needs a credit in Settings › About.

| Library | Version | What for |
|---|---|---|
| AndroidX Activity, AndroidX Core | 1.9.3, 1.15.0 | The screens, the Back button, edge-to-edge layout |
| Kotlin standard library, kotlinx.serialization | 2.4.21, 1.11.0 | The app's language, and the JSON of saved computers and pairing calls |
| ZXing core, zxing-android-embedded (JourneyApps) | 3.4.1, 4.3.0 | Scanning the pairing QR code |
| What AndroidX and Kotlin bring with them: AndroidX Annotation, Arch Core, Collection, Concurrent, Interpolator, Lifecycle, SavedState, Startup, Profile Installer, Tracing, VersionedParcelable, kotlinx.coroutines, JetBrains' Java annotations and Guava's ListenableFuture | As Gradle resolves them (`./gradlew :app:dependencies`) | What Activity and Core are built on; also Apache-2.0 |
