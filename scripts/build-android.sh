#!/usr/bin/env bash
# AudioSync Android 构建脚本
# T106: 交叉编译 Rust 核心 → JNI cdylib → Gradle 打包 APK/AAB
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ANDROID_JNI="$ROOT/app-android/app/src/main/jniLibs"

echo "=== 1. Rust 核心（arm64-v8a / x86_64） ==="
command -v cargo-ndk >/dev/null 2>&1 || cargo install cargo-ndk
(cd "$ROOT" && cargo ndk -t arm64-v8a -t x86_64 -o "$ANDROID_JNI" build --release -p core-jni)

echo "=== 2. Gradle 打包 ==="
cd "$ROOT/app-android"
if [ ! -w . ]; then echo "app-android 只读，跳过 gradle"; exit 0; fi
if command -v gradle >/dev/null 2>&1; then GRADLE=gradle; else GRADLE=./gradlew; [ -x gradlew ] || GRADLE=""; fi
if [ -n "$GRADLE" ]; then
    "$GRADLE" assembleRelease   # APK
    "$GRADLE" bundleRelease     # AAB
    echo ">>> APK:  app/build/outputs/apk/release/"
    echo ">>> AAB:  app/build/outputs/bundle/release/"
else
    echo "未找到 gradle/gradlew，仅完成 Rust .so 产出："
    find "$ANDROID_JNI" -name "*.so"
fi
