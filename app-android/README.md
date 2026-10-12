# app-android

Android 端壳（Kotlin + JNI），对应 PRD 平台适配。

## 结构

```
app-android/
├── src/main/
│   ├── java/com/audiosync/    # Kotlin UI 层
│   └── jniLibs/                # Rust 核心编译产物（.so）
└── README.md
```

## 说明

- 本目录为 T01 占位骨架，实际实现见 M10（T101–T106）。
- Rust 核心（`core-*` crate）通过 JNI 暴露 ABI 供 Kotlin 调用。
- 不属于 Rust workspace member，使用独立的 Android 构建系统（Gradle）。
