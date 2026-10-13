#!/usr/bin/env bash
# AudioSync 三平台发行包统一构建（M11 · T112）
#
# 用法：
#   ./scripts/build-release.sh linux     # Linux 二进制 + systemd + 前端资源
#   ./scripts/build-release.sh windows   # Windows MSI（调用 build-windows.sh）
#   ./scripts/build-release.sh android   # Android APK/AAB（调用 build-android.sh）
#   ./scripts/build-release.sh all       # 当前主机可构建的全部平台
#
# 产物：release/<platform>/ 目录，每个包内含 LICENSE 与第三方许可清单。
# 版本标签：与 workspace 版本一致，git tag v<version>（如 v0.1.0）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep -m1 '^version' Cargo.toml | tr -d ' "version=' || echo "0.1.0")"
VERSION="${VERSION:-0.1.0}"
OUT="release"

info() { echo "=== $* ==="; }

# 为发行目录附加合规文件（T111/T113 退出条件：LICENSE 随发行包提供）
attach_compliance() {
    local dir="$1"
    cp LICENSE "$dir/LICENSE"
    mkdir -p "$dir/docs"
    cp docs/third-party-licenses.md "$dir/docs/"
    cp docs/compliance.md "$dir/docs/"
}

build_linux() {
    info "Linux 构建（x86_64）"
    cargo build --release -p app-daemon -p app-desktop

    local dir="$OUT/linux-x86_64"
    rm -rf "$dir" && mkdir -p "$dir/systemd"

    cp target/release/audiosync "$dir/audiosync-daemon"
    cp target/release/audiosync-desktop "$dir/audiosync-desktop" 2>/dev/null \
        || echo "提示：app-desktop 未产出（Tauri GUI 可用 cargo tauri build 单独打包）"
    cp crates/app-daemon/systemd/audiosync.service "$dir/systemd/"
    cp -r dist "$dir/dist"

    attach_compliance "$dir"
    info "Linux 完成: $dir"
}

build_windows() {
    info "Windows 构建（交叉编译 x86_64-pc-windows-gnu）"
    cargo build --release --target x86_64-pc-windows-gnu -p app-daemon -p app-desktop

    local dir="$OUT/windows-x86_64"
    rm -rf "$dir" && mkdir -p "$dir"

    cp target/x86_64-pc-windows-gnu/release/audiosync.exe "$dir/audiosync-daemon.exe"
    if [ -f target/x86_64-pc-windows-gnu/release/audiosync-desktop.exe ]; then
        cp target/x86_64-pc-windows-gnu/release/audiosync-desktop.exe "$dir/"
    fi

    attach_compliance "$dir"
    info "Windows 完成: $dir"
}

build_android() {
    info "Android 构建（arm64-v8a / x86_64）"
    # 1) Rust 核心 .so（需 ANDROID_NDK_HOME）
    if [ -n "${ANDROID_NDK_HOME:-}" ] || command -v cargo-ndk >/dev/null 2>&1; then
        cargo ndk -t arm64-v8a -t x86_64 -o app-android/app/src/main/jniLibs build --release -p core-jni
    else
        echo "警告：未检测到 NDK/cargo-ndk，跳过 .so 构建"
    fi
    # 2) Gradle 打包（需 ANDROID_HOME + JDK17+）
    if command -v gradle >/dev/null 2>&1; then
        (cd app-android && gradle assembleRelease bundleRelease)
    fi

    local dir="$OUT/android"
    rm -rf "$dir" && mkdir -p "$dir"
    find "$ROOT/app-android/app/build/outputs" -type f \( -name "*.apk" -o -name "*.aab" \) \
        -exec cp {} "$dir/" \; 2>/dev/null || true
    # 无论 APK 是否产出，附上 JNI 库与合规文件
    if [ -d "$ROOT/app-android/app/src/main/jniLibs/arm64-v8a" ]; then
        mkdir -p "$dir/jniLibs" && cp -r "$ROOT/app-android/app/src/main/jniLibs/"* "$dir/jniLibs/"
    fi
    if [ -z "$(ls -A "$dir" 2>/dev/null)" ]; then
        echo "提示：未找到 APK/AAB，请先安装 Android SDK 后执行 gradle assembleRelease bundleRelease"
    fi
    attach_compliance "$dir"
    info "Android 完成: $dir"
}

print_tag() {
    info "版本标签"
    echo "工作区版本: v$VERSION"
    echo "发布时执行: git tag -a v$VERSION -m \"AudioSync v$VERSION\" && git push origin v$VERSION"
}

case "${1:-all}" in
    linux)   build_linux ;;
    windows) build_windows ;;
    android) build_android ;;
    all)
        case "$(uname -s)" in
            Linux)  build_linux; print_tag ;;
            Darwin) print_tag; echo "macOS 构建请在对应平台执行" ;;
        esac
        ;;
    *) echo "用法: $0 {linux|windows|android|all}"; exit 1 ;;
esac
