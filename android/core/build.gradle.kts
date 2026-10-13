plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.kotlin.serialization)
}

// Java 11 bytecode, which every Android version the app supports runs.
java {
    sourceCompatibility = JavaVersion.VERSION_11
    targetCompatibility = JavaVersion.VERSION_11
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_11
        allWarningsAsErrors = true
    }
}

// The pairing calls set the Host header to the name the person gave while
// connecting to the checked address; the JDK's HttpURLConnection drops a
// Host header unless told to keep it (Android's keeps it as it is).
tasks.test {
    systemProperty("sun.net.http.allowRestrictedHeaders", "true")
}

dependencies {
    implementation(libs.kotlinx.serialization.json)
    testImplementation(libs.junit)
}
