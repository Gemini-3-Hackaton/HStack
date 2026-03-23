#!/usr/bin/env zsh

setopt no_nomatch

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

resolve_android_home() {
	if [[ -n "${ANDROID_HOME:-}" ]]; then
		printf '%s\n' "$ANDROID_HOME"
		return
	fi

	if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
		printf '%s\n' "$ANDROID_SDK_ROOT"
		return
	fi

	if [[ -d "$HOME/Library/Android/sdk" ]]; then
		printf '%s\n' "$HOME/Library/Android/sdk"
		return
	fi

	if [[ -d "$HOME/Android/Sdk" ]]; then
		printf '%s\n' "$HOME/Android/Sdk"
		return
	fi

	printf '%s\n' "$HOME/Library/Android/sdk"
}

resolve_latest_child_dir() {
	local parent_dir="$1"
	local latest_name

	if [[ ! -d "$parent_dir" ]]; then
		return
	fi

	latest_name="$({
		for child_dir in "$parent_dir"/*(/N); do
			basename "$child_dir"
		done
	} | awk -F. '{ printf "%08d.%08d.%08d.%08d %s\n", $1, $2, $3, $4, $0 }' | sort | tail -n 1 | cut -d' ' -f2-)"

	if [[ -n "$latest_name" ]]; then
		printf '%s\n' "$parent_dir/$latest_name"
	fi
}

resolve_java_home() {
	if [[ -n "${JAVA_HOME:-}" ]]; then
		printf '%s\n' "$JAVA_HOME"
		return
	fi

	if [[ -x "/usr/libexec/java_home" ]]; then
		local detected_java_home
		detected_java_home="$(/usr/libexec/java_home -v 21 2>/dev/null || true)"
		if [[ -n "$detected_java_home" ]]; then
			printf '%s\n' "$detected_java_home"
			return
		fi
	fi

	if [[ -d "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home" ]]; then
		printf '%s\n' "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
		return
	fi

	if [[ -d "/usr/local/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home" ]]; then
		printf '%s\n' "/usr/local/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
		return
	fi
}

_resolved_android_home="$(resolve_android_home)"
_resolved_java_home="$(resolve_java_home)"
_resolved_ndk_home="${NDK_HOME:-$(resolve_latest_child_dir "$_resolved_android_home/ndk")}" 
_resolved_build_tools="${HSTACK_ANDROID_BUILD_TOOLS:-$(resolve_latest_child_dir "$_resolved_android_home/build-tools")}" 

export ANDROID_HOME="$_resolved_android_home"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export NDK_HOME="$_resolved_ndk_home"
export JAVA_HOME="$_resolved_java_home"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"

export HSTACK_ANDROID_ROOT="$ROOT_DIR"
export HSTACK_ANDROID_APP_DIR="$ROOT_DIR/crates/hstack-app"
export HSTACK_ANDROID_UNSIGNED_APK="$HSTACK_ANDROID_APP_DIR/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
export HSTACK_ANDROID_SIGNED_APK="$HSTACK_ANDROID_APP_DIR/gen/android/app/build/outputs/apk/universal/release/app-universal-release-debugsigned.apk"
export HSTACK_ANDROID_AAB="$HSTACK_ANDROID_APP_DIR/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab"
export HSTACK_ANDROID_APP_ID="${APP_ID:-com.hstack.app}"
export HSTACK_ANDROID_KEYSTORE="${HSTACK_ANDROID_KEYSTORE:-$HOME/.android/debug.keystore}"
export HSTACK_ANDROID_KEY_ALIAS="${HSTACK_ANDROID_KEY_ALIAS:-androiddebugkey}"
export HSTACK_ANDROID_KEY_PASSWORD="${HSTACK_ANDROID_KEY_PASSWORD:-android}"
export HSTACK_ANDROID_BUILD_TOOLS="$_resolved_build_tools"