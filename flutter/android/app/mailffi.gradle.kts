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
// The NDK location resolves as: ANDROID_NDK_HOME wins, otherwise
// <sdk.dir>/ndk/<flutter.ndkVersion> (the same version `ndkVersion =
// flutter.ndkVersion` pins in build.gradle.kts).

import org.gradle.api.tasks.Exec

val workspaceDir = file("${rootDir}/../..")
val jniLibsDir = file("${projectDir}/src/main/jniLibs")

// arm64 covers every current device; armv7 is old hardware and x86_64 is the
// emulator. Debug builds skip the two that only slow the loop down.
val releaseAbis = listOf("arm64-v8a", "armeabi-v7a", "x86_64")
val debugAbis = listOf("arm64-v8a", "x86_64")

fun resolveNdkDir(): String {
    // 1. Explicit env wins — CI and non-standard SDK layouts.
    System.getenv("ANDROID_NDK_HOME")?.takeIf { it.isNotBlank() }?.let { return it }
    // 2. Derive from the SDK location so no machine-specific path is baked in.
    val localProps = java.util.Properties()
    val localPropsFile = rootProject.file("local.properties")
    if (localPropsFile.exists()) {
        java.io.FileInputStream(localPropsFile).use(localProps::load)
    }
    val sdkDir = System.getenv("ANDROID_SDK_ROOT")
        ?: System.getenv("ANDROID_HOME")
        ?: localProps.getProperty("sdk.dir")
        ?: throw GradleException(
            "Cannot find Android NDK: ANDROID_NDK_HOME is unset and no " +
            "sdk.dir in android/local.properties. Set ANDROID_NDK_HOME or " +
            "install the NDK via SDK Manager."
        )
    // 3. Prefer the Flutter-pinned version so this stays in sync with
    // `ndkVersion = flutter.ndkVersion` in build.gradle.kts (handed over via
    // the mailffiNdkVersion extra property, as the `flutter` extension is not
    // visible from an applied script).
    val pinned = findProperty("mailffiNdkVersion") as? String
    if (pinned != null) {
        val dir = file("$sdkDir/ndk/$pinned")
        if (dir.isDirectory) return dir.absolutePath
        throw GradleException(
            "NDK $pinned (flutter.ndkVersion) not found at ${dir.absolutePath}. " +
            "Install it via SDK Manager or set ANDROID_NDK_HOME."
        )
    }
    return file("$sdkDir/ndk").listFiles()?.filter { it.isDirectory }
        ?.maxByOrNull { it.name }?.absolutePath
        ?: throw GradleException(
            "No NDK found under $sdkDir/ndk. Install one via SDK Manager " +
            "or set ANDROID_NDK_HOME."
        )
}

fun registerCargoNdk(name: String, abis: List<String>, profileArgs: List<String>) =
    tasks.register<Exec>(name) {
        group = "build"
        description = "Builds libmailffi.so for ${abis.joinToString(", ")}"
        workingDir = workspaceDir
        val cargoPath = System.getenv("PATH") ?: ""
        val extraPath = "${System.getProperty("user.home")}/.cargo/bin"
        if (!cargoPath.split(":").contains(extraPath)) {
            environment("PATH", "${extraPath}:${cargoPath}")
        }
        environment("ANDROID_NDK_HOME", resolveNdkDir())
        commandLine(
            listOf("cargo", "ndk") +
                abis.flatMap { listOf("-t", it) } +
                listOf("-o", jniLibsDir.absolutePath, "build", "-p", "mailffi") +
                profileArgs,
        )
        // A missing cargo-ndk is a setup problem with a one-line fix, so say
        // so rather than letting Gradle report a bare non-zero exit.
        isIgnoreExitValue = false
        // Drop the previous copy first: when cargo has nothing to rebuild,
        // cargo-ndk leaves an existing .so in place, so a release build after
        // `flutter run` packaged the ~150 MB unoptimised debug core.
        doFirst {
            jniLibsDir.mkdirs()
            abis.forEach { file("$jniLibsDir/$it/libmailffi.so").delete() }
        }
    }

val buildMailffiDebug = registerCargoNdk("buildMailffiDebug", debugAbis, emptyList())
val buildMailffiRelease =
    registerCargoNdk("buildMailffiRelease", releaseAbis, listOf("--release"))

// `mergeDebugJniLibFolders` and friends are what actually pick the .so files
// up, so the cargo build has to be finished before they run.
tasks.matching { it.name.matches(Regex("merge.*JniLibFolders")) }.configureEach {
    dependsOn(if (name.contains("Release")) buildMailffiRelease else buildMailffiDebug)
}
