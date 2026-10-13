# app-android

Android 端（Kotlin + JNI），对应 dev plan M10（T101–T106）。

## 结构

```
app-android/
├── app/
│   ├── build.gradle.kts               # T106 打包配置（arm64-v8a / x86_64）
│   └── src/main/
│       ├── AndroidManifest.xml        # T103 前台服务声明 / T105 按键接收器
│       ├── java/com/audiosync/
│       │   ├── JniBridge.kt           # T101 JNI 绑定（core-jni 的 JSON 协议）
│       │   ├── AudioSyncApp.kt        # 通知渠道初始化
│       │   ├── MainActivity.kt        # T104 界面（对齐桌面端）
│       │   ├── BatteryOptimizationHelper.kt  # T103 白名单引导，拒绝时降级
│       │   ├── saf/SafHelper.kt       # T102 SAF 授权与扫描
│       │   ├── playback/PlaybackService.kt   # T103 mediaPlayback + T105 媒体通知
│       │   ├── playback/HeadsetButtonReceiver.kt  # T105 耳机按键
│       │   └── transfer/TransferService.kt   # T103 dataSync
│       ├── res/                       # 布局、字符串、自适应图标
│       └── jniLibs/<abi>/libaudiosync_core.so  # Rust 产物（cargo ndk 生成）
├── build.gradle.kts
├── settings.gradle.kts
└── gradle.properties
```

## 构建

```bash
# 1. Rust 核心 → JNI cdylib（需 cargo-ndk 与 Android NDK）
../scripts/build-android.sh
# 2. APK / AAB（需 Android SDK）
cd app-android && ./gradlew assembleRelease bundleRelease
```

## 正式签名

- 密钥：`app-android/keystore/audiosync-release.jks`（RSA 2048，有效期 30 年），
  密码配置在同级 `audiosync-release.properties`。**两者均被 .gitignore 排除，绝不入库。**
- Gradle 自动检测：`keystore/` 目录存在则 `assembleRelease`/`bundleRelease`
  直接产出签名产物；缺失时回退为未签名 release（CI 友好）。
- 备份要求：密钥丢失将无法为同一 `applicationId` 发布更新。请至少两份
  独立备份（密码管理器/加密介质），并记录证书 SHA-256 指纹：
  `dc799a17dfe29c205042ff89c484c266cb3f633053cb361d929beb5b2beddf48`
- 重新生成（仅首次或彻底轮换时）：

  ```bash
  keytool -genkeypair -v -keystore app-android/keystore/audiosync-release.jks \
    -alias audiosync -keyalg RSA -keysize 2048 -validity 10950 \
    -dname "CN=AudioSync Release, OU=Mobile, O=AudioSync"
  # 再手工创建 keystore/audiosync-release.properties：
  # storeFile=audiosync-release.jks
  # storePassword=<随机密码>  keyAlias=audiosync  keyPassword=<同密码>
  ```

## 设计要点

- **协议复用**：Kotlin 通过 JSON 请求/响应与 Rust 核心通信，复用 `core-ipc`
  协议（与桌面 CLI/daemon 一致），JNI 层仅做字符串转换。
- **SAF（T102）**：`ACTION_OPEN_DOCUMENT_TREE` 授权音频库目录，无需存储权限；
  权限持久化，目录不可访问时标记失效（FR-01 边界）。
- **前台服务（T103）**：播放用 `mediaPlayback`，传输用 `dataSync`；
  电池优化白名单被拒绝时降级为仅前台运行，不崩溃（退出条件）。
- **媒体控制（T105）**：MediaSessionCompat 提供媒体通知与耳机按键
  （播放/暂停/上下曲/跳转）。
- Rust 侧逻辑在 `crates/core-jni`，与 JVM 解耦、可单元测试（9 测试）。
