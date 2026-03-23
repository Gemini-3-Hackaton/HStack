#!/usr/bin/env zsh

set -euo pipefail
setopt no_nomatch

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

source "$SCRIPT_DIR/android-env.sh"

TAURI_BIN="$ROOT_DIR/node_modules/.bin/tauri"
APKSIGNER_BIN="$HSTACK_ANDROID_BUILD_TOOLS/apksigner"
KEYTOOL_BIN="$JAVA_HOME/bin/keytool"

log() {
  printf '\n[%s] %s\n' "android-reinstall" "$1"
}

fail() {
  printf '\n[%s] %s\n' "android-reinstall" "$1" >&2
  exit 1
}

require_file() {
  local path="$1"
  local label="$2"
  [[ -f "$path" ]] || fail "$label not found at $path"
}

require_dir() {
  local path="$1"
  local label="$2"
  [[ -d "$path" ]] || fail "$label not found at $path"
}

require_cmd() {
  local command_name="$1"
  command -v "$command_name" >/dev/null 2>&1 || fail "Required command not found: $command_name"
}

ensure_debug_keystore() {
  if [[ -f "$HSTACK_ANDROID_KEYSTORE" ]]; then
    return
  fi

  mkdir -p "$(dirname "$HSTACK_ANDROID_KEYSTORE")"

  log "Creating debug keystore at $HSTACK_ANDROID_KEYSTORE"
  "$KEYTOOL_BIN" -genkeypair \
    -v \
    -keystore "$HSTACK_ANDROID_KEYSTORE" \
    -storepass "$HSTACK_ANDROID_KEY_PASSWORD" \
    -alias "$HSTACK_ANDROID_KEY_ALIAS" \
    -keypass "$HSTACK_ANDROID_KEY_PASSWORD" \
    -keyalg RSA \
    -keysize 2048 \
    -validity 10000 \
    -dname "CN=Android Debug,O=Android,C=US" >/dev/null
}

ensure_single_connected_device() {
  local devices
  devices="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')"

  if [[ -z "$devices" ]]; then
    fail "No authorized adb device detected. Connect the phone, unlock it, enable USB debugging, and accept the trust prompt."
  fi

  local count
  count="$(printf '%s\n' "$devices" | sed '/^$/d' | wc -l | tr -d ' ')"
  if [[ "$count" != "1" ]]; then
    fail "Expected exactly one adb device, found $count. Disconnect extra devices or emulators first."
  fi
}

log "Checking Android toolchain"
require_dir "$ANDROID_HOME" "ANDROID_HOME"
require_dir "$NDK_HOME" "NDK_HOME"
require_dir "$JAVA_HOME" "JAVA_HOME"
require_file "$TAURI_BIN" "Tauri CLI"
require_file "$APKSIGNER_BIN" "Android apksigner"
require_file "$KEYTOOL_BIN" "Java keytool"
require_cmd adb
require_cmd npm

log "Restarting adb and checking connected device"
adb kill-server >/dev/null 2>&1 || true
adb start-server >/dev/null
ensure_single_connected_device
adb devices -l

log "Building Android app"
cd "$HSTACK_ANDROID_APP_DIR"
"$TAURI_BIN" android build

require_file "$HSTACK_ANDROID_UNSIGNED_APK" "Unsigned APK"

ensure_debug_keystore

log "Signing rebuilt APK"
rm -f "$HSTACK_ANDROID_SIGNED_APK" "$HSTACK_ANDROID_SIGNED_APK.idsig"
cp "$HSTACK_ANDROID_UNSIGNED_APK" "$HSTACK_ANDROID_SIGNED_APK"

"$APKSIGNER_BIN" sign \
  --ks "$HSTACK_ANDROID_KEYSTORE" \
  --ks-key-alias "$HSTACK_ANDROID_KEY_ALIAS" \
  --ks-pass "pass:$HSTACK_ANDROID_KEY_PASSWORD" \
  --key-pass "pass:$HSTACK_ANDROID_KEY_PASSWORD" \
  "$HSTACK_ANDROID_SIGNED_APK"

"$APKSIGNER_BIN" verify "$HSTACK_ANDROID_SIGNED_APK"

log "Installing APK on connected device"
adb install -r "$HSTACK_ANDROID_SIGNED_APK"

log "Launching app"
adb shell monkey -p "$HSTACK_ANDROID_APP_ID" -c android.intent.category.LAUNCHER 1 >/dev/null

log "Done"
printf 'APK: %s\n' "$HSTACK_ANDROID_SIGNED_APK"