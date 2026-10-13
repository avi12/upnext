// The Kotlin plugins load here, once for the whole build, so the Android
// plugin's built-in Kotlin uses the same version as `core`. The Android
// plugin loads here too: Kotlin's plugin refers to the Android plugin's
// classes, which it can only see when both share this class loader.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}
