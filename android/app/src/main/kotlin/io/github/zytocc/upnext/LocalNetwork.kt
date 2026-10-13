package io.github.zytocc.upnext

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

/**
 * Android 17's local network protection: an
 * app that targets API 37 reaches the home network only with the
 * `ACCESS_LOCAL_NETWORK` permission (Nearby devices), except for the
 * computers the system's picker found for it. UpNext finds computers
 * through the picker, so the permission is asked for only before an
 * address typed or scanned that may be on this network.
 */
object LocalNetwork {
    /** `Manifest.permission.ACCESS_LOCAL_NETWORK`, written out so older
     *  versions' builds don't inline a constant they don't have. */
    const val PERMISSION = "android.permission.ACCESS_LOCAL_NETWORK"

    /** Whether this device holds the home network back from the app until
     *  the person allows Nearby devices: Android 17 and later, for an app
     *  that targets it. */
    fun held(context: Context): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.CINNAMON_BUN &&
            context.applicationInfo.targetSdkVersion >= Build.VERSION_CODES.CINNAMON_BUN &&
            context.checkSelfPermission(PERMISSION) != PackageManager.PERMISSION_GRANTED
}
