import java.util.Properties
import java.io.FileInputStream

plugins {
    id("com.android.application")
    // No org.jetbrains.kotlin.android: AGP 9 has Kotlin support built in
    // (it fails the build if the plugin is still applied). The Compose
    // compiler plugin below is still needed.
    id("org.jetbrains.kotlin.plugin.compose")
}

val keystorePropertiesFile = rootProject.file("key.properties")
val keystoreProperties = Properties()
if (keystorePropertiesFile.exists()) {
    keystoreProperties.load(FileInputStream(keystorePropertiesFile))
}

android {
    // The namespace is JNI-bound: crates/mailffi/src/android.rs exports
    // Java_de_renier_mailclient_MailNative_* symbols, so the MailNative
    // object must stay in package de.renier.mailclient. Never rename this
    // (the applicationId below is free to change — JNI does not see it).
    namespace = "de.renier.mailclient"
    compileSdk = 36
    // Pinned: must match an NDK installed under <sdk>/ndk (see README).
    ndkVersion = "28.2.13676358"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        isCoreLibraryDesugaringEnabled = true
    }

    defaultConfig {
        // Transition id so this app installs next to the Flutter one while
        // both exist. Drop the suffix at the flip, when it replaces it.
        // res/xml/shortcuts.xml repeats it as targetPackage: change both.
        applicationId = "de.renier.mailclient.native"
        // Mirror of the workspace root Cargo.toml version (see AGENTS.md
        // bump order); fresh versionCode track for the new applicationId.
        versionCode = 6
        versionName = "0.11.0"
        minSdk = 24
        targetSdk = 36
    }

    signingConfigs {
        create("release") {
            if (keystorePropertiesFile.exists()) {
                keyAlias = keystoreProperties["keyAlias"] as String?
                keyPassword = keystoreProperties["keyPassword"] as String?
                storeFile = keystoreProperties["storeFile"]?.let { file(it as String) }
                storePassword = keystoreProperties["storePassword"] as String?
            } else {
                // Loud on purpose: a release signed with the public debug key
                // is exactly what Play Protect flags as harmful. build.sh
                // refuses this combination outright; this fallback only exists
                // so debug builds keep working on machines without a keystore.
                logger.warn("No android/key.properties -- signing the release with DEBUG keys. See android/README.md.")
                keyAlias = signingConfigs.getByName("debug").keyAlias
                keyPassword = signingConfigs.getByName("debug").keyPassword
                storeFile = signingConfigs.getByName("debug").storeFile
                storePassword = signingConfigs.getByName("debug").storePassword
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    buildFeatures {
        compose = true
    }

    lint {
        abortOnError = false
        checkReleaseBuilds = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.4")
    // WorkManager for the native background checks (MailCheckWorker.kt,
    // enqueued by the periodic schedule and every alarm shot).
    implementation("androidx.work:work-runtime:2.11.2")
    // No explicit androidx.core: the FileProvider for the attachment viewer
    // copies (reader_paths.xml) arrives transitively, and the newest core
    // line already wants compileSdk 37 (see README).

    // Jetpack Compose (approved UI toolkit for the native frontend).
    // Versions come from the BOM; the compiler is the Kotlin plugin above.
    val composeBom = platform("androidx.compose:compose-bom:2026.04.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    // androidx.activity is versioned outside the Compose BOM.
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.runtime:runtime")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

// Builds the Rust core into src/main/jniLibs before Gradle packages it
// (adapted from flutter/android/app/mailffi.gradle.kts; the only difference
// is where the pinned NDK version comes from — here it is the ndkVersion
// above, handed over via the mailffiNdkVersion extra property).
extra["mailffiNdkVersion"] = "28.2.13676358"
apply(from = "mailffi.gradle.kts")
