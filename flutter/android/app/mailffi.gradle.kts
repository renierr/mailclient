// Builds the Rust core for Android and drops one `libmailffi.so` per ABI
// where Gradle packages JNI libraries from.
//
// Applied from `build.gradle.kts`. Kept separate so the generated Flutter
// Gradle file stays close to what `flutter create` produces and this stays
// easy to read as one thing.
//
// Prerequisites, none of which this file installs for you:
//   * the Android NDK (Android Studio: SDK Manager → SDK Tools → NDK)
//   * `cargo install cargo-ndk`
//   * `rustup target add aarch64-linux-android armv7-linux-androideabi \
//        x86_64-linux-android`
//
// It also does not yet work: `mailcore` depends on the `keyring` crate only
// on Linux, Windows and macOS, so `mailcore::auth` has no backend to compile
// against for Android. See `flutter/README.md` ("Android") for what that
// needs. The wiring is here so that work is a `mailcore` change and not also
// a build-system change.

import org.gradle.api.tasks.Exec

val workspaceDir = file("${rootDir}/../..")
val jniLibsDir = file("${projectDir}/src/main/jniLibs")

// arm64 covers every current device; armv7 is old hardware and x86_64 is the
// emulator. Debug builds skip the two that only slow the loop down.
val releaseAbis = listOf("arm64-v8a", "armeabi-v7a", "x86_64")
val debugAbis = listOf("arm64-v8a", "x86_64")

fun registerCargoNdk(name: String, abis: List<String>, profileArgs: List<String>) =
    tasks.register<Exec>(name) {
        group = "build"
        description = "Builds libmailffi.so for ${abis.joinToString(", ")}"
        workingDir = workspaceDir
        commandLine(
            listOf("cargo", "ndk") +
                abis.flatMap { listOf("-t", it) } +
                listOf("-o", jniLibsDir.absolutePath, "build", "-p", "mailffi") +
                profileArgs,
        )
        // A missing cargo-ndk is a setup problem with a one-line fix, so say
        // so rather than letting Gradle report a bare non-zero exit.
        isIgnoreExitValue = false
        doFirst { jniLibsDir.mkdirs() }
    }

val buildMailffiDebug = registerCargoNdk("buildMailffiDebug", debugAbis, emptyList())
val buildMailffiRelease =
    registerCargoNdk("buildMailffiRelease", releaseAbis, listOf("--release"))

// `mergeDebugJniLibFolders` and friends are what actually pick the .so files
// up, so the cargo build has to be finished before they run.
tasks.matching { it.name.matches(Regex("merge.*JniLibFolders")) }.configureEach {
    dependsOn(if (name.contains("Release")) buildMailffiRelease else buildMailffiDebug)
}
