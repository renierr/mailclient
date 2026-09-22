# Builds the Rust core and ships it next to the Flutter executable.
#
# Included from both `windows/CMakeLists.txt` and `linux/CMakeLists.txt`, so
# the two desktop targets cannot drift apart. Android does not come through
# here — Gradle drives `cargo-ndk` instead; see `android/app/build.gradle.kts`.
#
# The library is built into the workspace's own `target/` rather than into the
# CMake build tree, so a `flutter run` and a `cargo test` share one compilation
# cache instead of each paying for the other's rebuild.

set(MAILFFI_WORKSPACE "${CMAKE_CURRENT_SOURCE_DIR}/../..")

# The core is built optimised even for a debug Flutter build, deliberately.
# It is not the code being debugged — an unoptimised bundled SQLite, rustls
# and MIME parser make sync slow enough to change how the app behaves, and
# Dart-side debugging is unaffected either way. Set MAILFFI_DEBUG=ON to get an
# unoptimised core when it is the Rust that needs stepping through.
option(MAILFFI_DEBUG "Build the Rust core unoptimised, for debugging it" OFF)
if(MAILFFI_DEBUG)
  set(MAILFFI_CARGO_FLAGS "")
  set(MAILFFI_PROFILE_DIR "debug")
else()
  set(MAILFFI_CARGO_FLAGS "--release")
  set(MAILFFI_PROFILE_DIR "release")
endif()

if(WIN32)
  set(MAILFFI_LIB_NAME "mailffi.dll")
else()
  set(MAILFFI_LIB_NAME "libmailffi.so")
endif()

set(MAILFFI_LIB
    "${MAILFFI_WORKSPACE}/target/${MAILFFI_PROFILE_DIR}/${MAILFFI_LIB_NAME}")

# Always run cargo: it is its own up-to-date check and costs almost nothing
# when there is nothing to do, whereas teaching CMake the Rust dependency
# graph would mean maintaining a second, wrong copy of it.
add_custom_target(mailffi_build ALL
  COMMAND ${CMAKE_COMMAND} -E env cargo build -p mailffi ${MAILFFI_CARGO_FLAGS}
  WORKING_DIRECTORY "${MAILFFI_WORKSPACE}"
  COMMENT "Building the Rust mail core (mailffi, ${MAILFFI_PROFILE_DIR})"
  USES_TERMINAL
  VERBATIM
)

# Into the bundle's library directory, which is where the engine looks at
# runtime and what the packaged app ships.
install(FILES "${MAILFFI_LIB}"
  DESTINATION "${INSTALL_BUNDLE_LIB_DIR}"
  COMPONENT Runtime
)
