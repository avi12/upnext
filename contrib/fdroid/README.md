# F-Droid

`io.github.zytocc.upnext.yml` is the Android app's recipe for F-Droid. F-Droid builds the app itself from a tag, in its own build server, with no network but its Maven mirrors; the app uses nothing from Google Play services, and every library it uses is Apache-2.0.

To submit it, once a `v*` tag has an `upnext-android-<version>.apk` on its GitHub release:

1. Check that `AllowedAPKSigningKeys` is the release certificate's SHA-256 (`apksigner verify --print-certs` on the release's APK prints it; lowercase, no colons), and that the build entry and `CurrentVersion` name that release.
2. Fork `gitlab.com/fdroid/fdroiddata`, copy the file to `metadata/io.github.zytocc.upnext.yml`, and check it there with `fdroid lint io.github.zytocc.upnext` and `fdroid build -v -l io.github.zytocc.upnext` (fdroidserver, in F-Droid's build container).
3. Open the merge request. F-Droid's reviewers may ask for changes; their scanner looks at the whole repository, so a file it flags outside `android/` goes under `scandelete` in the build entry.

`Binaries` makes F-Droid compare its own build with the release's APK and ship the release's signature, so the two install over each other. That needs the build to be reproducible: the app turns off minification and the version-control and dependency blocks that would differ (`android/app/build.gradle.kts`). If F-Droid's comparison still finds a difference, take `Binaries` and `AllowedAPKSigningKeys` out and F-Droid signs the app with its own key instead; an F-Droid install then can't update from the GitHub APK or the other way round.

New versions: `UpdateCheckMode: Tags` finds each `v*` tag, and `AutoUpdateMode: Version` adds its build entry, reading `versionName` and `versionCode` from `android/app/build.gradle.kts`, which `docs/dev/RELEASE.md` says to change with each release.
