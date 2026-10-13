# Android

The Android app uses UpNext on your computer from a phone, a tablet or an Android TV (Android 8 or later). UpNext itself still runs on the computer; the app finds it, pairs with it and shows its UI. A release carries it as `upnext-android-<version>.apk`, with its `.sha256`, when the app's signing key is set up for it; a release without one has no APK.

1. On the computer, turn on **Allow other devices** (Settings › Devices and API).
2. On the phone, download the APK from a [release](https://github.com/zytocc/upnext/releases) and open it. The first time, Android asks you to let the app you downloaded it with (the browser or Files) install apps; allow it, then install. To check the file first, on a computer: `sha256sum -c upnext-android-<version>.apk.sha256`.
3. Open UpNext on the phone. It lists the computers on your network that run UpNext; tap yours. The computer shows "A phone called … wants to use UpNext" with a four-digit number. Check the phone shows the same one and choose **Allow** there. The phone opens UpNext, and goes straight to it from then on.

Away from home, or on a network that hides the computer, choose **Add by address** (for example `https://den.tailnet.ts.net`), or **Scan the QR code** from Settings › Devices and API, Add a device, which pairs with the code it carries. "Type a code instead" takes the same six digits.

Several computers can be kept. **Forget** removes one from the phone and asks that computer to remove the phone from its devices. The phone keeps each computer's token sealed with a key in Android's keystore, and leaves it out of backups and transfers to a new phone, which asks again.

Games start on the computer, and Play on the phone says so. On an Android TV, UpNext is in the launcher's apps and opens in [big screen](../big-screen.md), for the remote or a gamepad.

[Other devices and scripts](../other-devices.md) covers using UpNext from another device's browser.
