// UpNext's Android app: a thin native app around the
// web UI. `core` is plain Kotlin (addresses, the announcement, saved
// computers and the pairing calls) with tests that run on any JVM; `app`
// is the Android app itself.

// Google's repository serves the Android plugin and AndroidX alone, so a
// build asks it for nothing else.
pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
    }
}

rootProject.name = "upnext-android"

include(":core")
// `-Pupnext.coreOnly=true` leaves the Android app out, so `core`'s tests
// run on a machine that can't reach Google's Maven repository or has no
// Android SDK. The root build file then declares no Android plugin either:
// Gradle resolves the root's plugins before it knows `app` is left out.
if (providers.gradleProperty("upnext.coreOnly").orNull == "true") {
    rootProject.buildFileName = "core-only.gradle.kts"
} else {
    include(":app")
}
