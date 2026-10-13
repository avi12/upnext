// The root build file with `-Pupnext.coreOnly=true` (settings.gradle.kts
// chooses it): build.gradle.kts without the Android plugin, which Gradle
// would fetch from Google's Maven repository even with the app left out.
// Keep the Kotlin plugins in step with build.gradle.kts.
plugins {
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}
