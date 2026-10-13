import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
}

// The release key comes from the environment, in the release workflow on a
// tag only (docs/dev/RELEASE.md, "The Android app"). Without it a release
// build is unsigned, which is what `ci` and F-Droid's own build want.
val releaseKeystore = providers.environmentVariable("UPNEXT_ANDROID_KEYSTORE").orNull

android {
    namespace = "io.github.zytocc.upnext"
    compileSdk = 37

    defaultConfig {
        applicationId = "io.github.zytocc.upnext"
        minSdk = 26
        targetSdk = 37
        // versionName is UpNext's version (Cargo.toml; `checkVersion`
        // fails when they differ), and versionCode is worked out from it:
        // major × 1,000,000 + minor × 1,000 + patch, so 0.1.0 is 1000.
        // Both stay literal here, where F-Droid's update check reads them.
        versionCode = 1000
        versionName = "0.1.0-rc.2"
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = providers.environmentVariable("UPNEXT_ANDROID_KEYSTORE_PASSWORD").get()
                keyAlias = providers.environmentVariable("UPNEXT_ANDROID_KEY_ALIAS").get()
                keyPassword = providers.environmentVariable("UPNEXT_ANDROID_KEY_PASSWORD").get()
            }
        }
    }

    buildTypes {
        release {
            // No shrinking: the app is small, and an APK built the same way
            // from the same tag is what F-Droid's reproducible build checks.
            isMinifyEnabled = false
            if (releaseKeystore != null) signingConfig = signingConfigs.getByName("release")
            // Git details in the APK would differ between F-Droid's checkout
            // and the release workflow's.
            vcsInfo.include = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }

    // Google's encrypted block of dependency details, which only Play can
    // read; F-Droid asks for it to be left out.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_11
    }
}

dependencies {
    implementation(project(":core"))
    implementation(libs.androidx.activity)
    implementation(libs.androidx.core)
    implementation(libs.zxing.android.embedded)
}

// The app's version must be UpNext's, read from the repository's Cargo.toml.
val cargoToml = rootProject.layout.projectDirectory.file("../Cargo.toml")
tasks.register("checkVersion") {
    val appVersion = android.defaultConfig.versionName
    val appCode = android.defaultConfig.versionCode
    val cargo = providers.fileContents(cargoToml).asText
    doLast {
        val version = checkNotNull(Regex("""(?m)^version\s*=\s*"([^"]+)"""").find(cargo.get())?.groupValues?.get(1)) { "Cargo.toml has no version" }
        check(version == appVersion) { "versionName is $appVersion but Cargo.toml says $version: change both, and versionCode" }
        val (major, minor, patch) = version.substringBefore('-').split('.').map(String::toInt)
        check(appCode == major * 1_000_000 + minor * 1_000 + patch) { "versionCode $appCode doesn't match $version" }
    }
}
