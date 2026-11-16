#!/bin/sh
# KYNEX for Android: KYNEX built for the device (arm64) and put in the app
# (android/), the app built: an APK to install.
#
#   tools/android.sh            a release APK (signed as a debug build is,
#                               unless KYNEX_KEYSTORE and its passwords are
#                               given: see android/app/build.gradle)
#   tools/android.sh install    and installed on the device or emulator
#                               connected (adb)
#
# Needs the Android SDK with an NDK (ANDROID_HOME, or ~/Library/Android/sdk,
# ~/Android/Sdk), a JDK 17 (JAVA_HOME, or Homebrew's openjdk@17) and the
# Rust target: rustup target add aarch64-linux-android.
set -e
cd "$(dirname "$0")/.."

SDK="${ANDROID_HOME:-}"
for d in "$HOME/Library/Android/sdk" "$HOME/Android/Sdk"; do
  [ -z "$SDK" ] && [ -d "$d" ] && SDK="$d"
done
[ -d "$SDK" ] || { echo "no Android SDK: set ANDROID_HOME" >&2; exit 1; }
NDK="${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* 2>/dev/null | sort | tail -1)}"
[ -d "$NDK" ] || { echo "no NDK in $SDK/ndk: install one (sdkmanager 'ndk;28.2.13676358')" >&2; exit 1; }
BIN="$(ls -d "$NDK"/toolchains/llvm/prebuilt/*/bin | head -1)"
if [ -z "$JAVA_HOME" ] && [ -d /opt/homebrew/opt/openjdk@17 ]; then
  export JAVA_HOME=/opt/homebrew/opt/openjdk@17
fi

# KYNEX itself, for Android 8 (API 26) and on. Its mesh code is C++: the
# NDK's own runtime linked in, none to carry beside it.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$BIN/aarch64-linux-android26-clang"
export CC_aarch64_linux_android="$BIN/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$BIN/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="$BIN/llvm-ar"
export CXXSTDLIB_aarch64_linux_android=c++_static
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-lc++abi"
cargo build --release -p kynex --bin kynex --target aarch64-linux-android

# A program, carried as a native library is: the one way an app may bring
# one to run.
LIBS=android/app/src/main/jniLibs/arm64-v8a
mkdir -p "$LIBS"
cp target/aarch64-linux-android/release/kynex "$LIBS/libkynex.so"

export ANDROID_HOME="$SDK"
(cd android && ./gradlew --quiet assembleRelease)
APK=android/app/build/outputs/apk/release/app-release.apk
ls -l "$APK"
if [ "$1" = install ]; then
  "$SDK/platform-tools/adb" install -r "$APK"
fi
