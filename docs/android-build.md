# HStack Android Build Guide

This document explains how to build the Android version of HStack from this repository, sign an APK for local device testing, and install it with adb.

## Scope

These instructions cover the public app in `crates/hstack-app`, which packages the React frontend from `frontend/` inside the Tauri Android shell.

The commands below were validated from the repository root.

## Project Layout

- `frontend/`: React/Vite UI
- `crates/hstack-app/`: Tauri app shell and native commands
- `crates/hstack-app/gen/android/`: generated Android project created by Tauri

## Prerequisites

You need all of the following installed:

- Rust stable toolchain
- Node.js and npm
- Android SDK
- Android NDK
- Java 21
- Tauri CLI via the repo dev dependency
- adb

### Required versions and notes

- Java 21 is required for the Android Gradle build used here. Java 25 failed with an unsupported class file version error.
- The Android build in this repo was validated with NDK `29.0.13846066`.
- `adb` is used both to detect the phone and install the generated APK.

## Repository Setup

Install JavaScript dependencies from the repo root:

```bash
npm install
npm install --prefix frontend
```

The Tauri app package also needs its own dependencies resolved:

```bash
npm install --prefix crates/hstack-app
```

## Android Environment Variables

Before running Android commands, export the Android SDK, NDK, and Java 21 paths.

Typical macOS example values:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export NDK_HOME="$ANDROID_HOME/ndk/29.0.13846066"
export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
```

If your SDK or JDK is installed somewhere else, adjust the paths accordingly.

The committed helper script is less strict than the examples above: it first respects explicit environment variables, then tries to auto-detect common Android SDK locations, the latest installed NDK, the latest installed build-tools directory, and Java 21.

## One-Time Android Project Generation

If `crates/hstack-app/gen/android/` does not exist yet, generate it once with:

```bash
cd crates/hstack-app
npm run tauri android init
```

This creates the Gradle project under `crates/hstack-app/gen/android/`.

## Build Commands

### Debug build

Use this for faster local iterations:

```bash
cd crates/hstack-app
../../node_modules/.bin/tauri android build
```

### Release build

This repository currently uses the same Tauri Android build command to produce the release artifacts consumed by the generated Gradle project:

```bash
cd crates/hstack-app
../../node_modules/.bin/tauri android build
```

The generated Android build internally invokes release profile Rust builds for the Android targets.

## Expected Output Files

After a successful release build, the main artifacts are:

- Unsigned APK: `crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk`
- Signed debug APK for local testing: `crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk`
- AAB: `crates/hstack-app/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab`

The application identifier is currently:

```text
com.hstack.app
```

That value comes from `crates/hstack-app/tauri.conf.json`.

## Signing an APK for Device Testing

The Tauri Android release build produced an unsigned APK. For direct installation on a phone, sign it with a debug keystore.

### 1. Create a debug keystore if needed

```bash
mkdir -p "$HOME/.android"

"$JAVA_HOME/bin/keytool" -genkeypair \
  -v \
  -keystore "$HOME/.android/debug.keystore" \
  -storepass android \
  -alias androiddebugkey \
  -keypass android \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 \
  -dname "CN=Android Debug,O=Android,C=US"
```

### 2. Copy and sign the unsigned APK

```bash
cp \
  crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk \
  crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk

"$ANDROID_HOME/build-tools/35.0.0/apksigner" sign \
  --ks "$HOME/.android/debug.keystore" \
  --ks-key-alias androiddebugkey \
  --ks-pass pass:android \
  --key-pass pass:android \
  crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk
```

### 3. Verify the signed APK

```bash
"$ANDROID_HOME/build-tools/35.0.0/apksigner" verify \
  crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk
```

## adb Device Checks

Check whether the Mac can see the phone:

```bash
adb devices -l
```

If the device list is empty:

- ensure the phone is unlocked
- enable Developer Options
- enable USB debugging
- set the USB mode to file transfer, not charge-only
- accept the RSA trust prompt on the phone when it appears
- reconnect the cable and rerun `adb devices -l`

If needed, restart the daemon:

```bash
adb kill-server
adb start-server
adb devices -l
```

## Install the APK on the Phone

Once the phone appears in `adb devices -l`, install the signed APK with:

```bash
adb install -r crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk
```

If you want to launch it immediately after installation:

```bash
adb shell monkey -p com.hstack.app -c android.intent.category.LAUNCHER 1
```

## Developer Reinstall Script

For the normal Android proof-of-concept loop, use the repo script instead of manually repeating the build, sign, install, and launch steps:

```bash
./scripts/android-reinstall-dev.sh
```

Or through the root npm shortcut:

```bash
npm run android:reinstall
```

What the script does:

- loads Android SDK, NDK, and Java configuration
- verifies the required tools exist
- builds the Android app
- prepares a debug keystore if needed
- signs the rebuilt APK
- checks for a connected adb device
- reinstalls the APK on the device
- launches the app

### Environment overrides

The script uses reasonable defaults for this machine, but you can override them with environment variables:

```bash
ANDROID_HOME=/path/to/sdk \
NDK_HOME=/path/to/ndk \
JAVA_HOME=/path/to/jdk21 \
APP_ID=com.hstack.app \
./scripts/android-reinstall-dev.sh
```

Nothing in the script is tied to your username or a single absolute repository path. The only machine-sensitive values are environment defaults for Android SDK, NDK, build-tools, and Java, and those are now overrideable and auto-detected when possible.

### Optional helper file

If you want to reuse the environment setup in your own shell session, source:

```bash
source ./scripts/android-env.sh
```

## Common Build Issues

### Gradle fails with unsupported class version

Cause:

- using a too-new JDK, especially Java 25

Fix:

- switch `JAVA_HOME` to Java 21

### Android build complains about missing SDK or NDK

Cause:

- `ANDROID_HOME`, `ANDROID_SDK_ROOT`, or `NDK_HOME` is unset or points to the wrong directory

Fix:

- export the correct paths before running the Tauri Android command

### `adb devices` shows nothing

Cause:

- phone not authorized, charge-only USB mode, or USB debugging disabled

Fix:

- unlock the phone, enable USB debugging, accept the trust prompt, and retry

### The build expects a `tauri` npm script

This repository already includes the required script in `crates/hstack-app/package.json`:

```json
{
  "scripts": {
    "tauri": "tauri"
  }
}
```

## Recommended Build Flow

For a clean Android test cycle:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export NDK_HOME="$ANDROID_HOME/ndk/29.0.13846066"
export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"

npm install
npm install --prefix frontend
npm install --prefix crates/hstack-app

cd crates/hstack-app
../../node_modules/.bin/tauri android build

cd ../..
adb devices -l
adb install -r crates/hstack-app/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk
```

Or just run:

```bash
npm run android:reinstall
```

## Current Status

At the time this document was written:

- the Android project had already been generated
- release artifacts were successfully produced
- a signed local-test APK was available
- adb was installed on macOS, but the phone was not yet visible to `adb devices -l`
