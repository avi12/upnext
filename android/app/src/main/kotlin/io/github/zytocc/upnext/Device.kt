package io.github.zytocc.upnext

import android.app.UiModeManager
import android.content.Context
import android.content.res.Configuration
import android.os.Build
import android.provider.Settings
import android.view.View
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

/** What this device tells the computer about itself when it asks. */
object Device {
    fun isTv(context: Context): Boolean =
        context.getSystemService(UiModeManager::class.java)?.currentModeType == Configuration.UI_MODE_TYPE_TELEVISION

    /** `phone`, `tablet` or `tv`, as `POST /pair/request` takes them. */
    fun kind(context: Context): String = when {
        isTv(context) -> "tv"
        context.resources.configuration.smallestScreenWidthDp >= 600 -> "tablet"
        else -> "phone"
    }

    /** The name the person gave the device in its settings, else its model,
     *  for "A phone called "Pixel 9" wants to use UpNext". */
    fun name(context: Context): String =
        Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)?.trim()?.takeIf { it.isNotEmpty() }
            ?: Build.MODEL?.trim()?.takeIf { it.isNotEmpty() }
            ?: "Android device"
}

/** The screens draw under the status and navigation bars (edge to edge, as
 *  Android 15 and later insist), so each pads its root by the bars and the
 *  display cutout, and the WebView by the keyboard too. */
fun padForSystemBars(view: View, keyboard: Boolean = false) {
    ViewCompat.setOnApplyWindowInsetsListener(view) { v, insets ->
        var types = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        if (keyboard) types = types or WindowInsetsCompat.Type.ime()
        val bars = insets.getInsets(types)
        v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
        WindowInsetsCompat.CONSUMED
    }
}
