# 架构与模块边界约定

本文档定义 Rust 工作区的分层与模块边界，对应 dev plan §1.1–1.2。

## 分层模型

| 层 | 包含 crate | 约束 |
|---|---|---|
| L0 核心层 | `core-store`, `core-library`, `core-playlist`, `core-player`, `core-position`, `core-cast`, `core-transfer`, `core-metadata`, `core-share`, `core-config`, `core-log` | 平台无关，三端共享；不得依赖 L1/L2 |
| L1 控制层 | `core-ipc`, `app-daemon` | 依赖 L0；不得依赖 L2 |
| L2 界面层 | `app-desktop`, `app-android` | 依赖 L0/L1；仅做展示与交互，业务逻辑下沉到 L0 |

## 依赖规则

1. **单向依赖**：L2 → L1 → L0，禁止反向依赖。
2. **核心层内部**：按业务领域划分，避免循环依赖。允许的依赖方向：
   - `core-store`（持久化底座，无业务依赖）
   - `core-library` → `core-store`, `core-playlist`
   - `core-player` → `core-position`
   - `core-transfer` → `core-cast`
   - `core-share` → `core-playlist`, `core-transfer`
   - `core-metadata` 独立
   - `core-config`, `core-log` 独立
3. **界面层**：`app-desktop`（Tauri）和 `app-android`（Kotlin+JNI）通过 `core-ipc` 或 JNI 调用核心层，不直接引用内部实现细节。

## crate 职责速查

| crate | 职责 | 对应需求 |
|---|---|---|
| `core-store` | 清单/位置/配置/元数据持久化与原子写入 | 7.3, FR-09 |
| `core-library` | 音频库、目录扫描、文件监听 | FR-01 |
| `core-playlist` | 清单生成、排序、自动维护 | FR-02 |
| `core-player` | 解码（Symphonia）、播放控制、音频输出 | FR-03 |
| `core-position` | 播放位置记录与续播 | FR-04 |
| `core-cast` | 设备发现、配对、设备状态机 | FR-05 |
| `core-transfer` | 直连/中继传输、断点续传、排队、校验 | FR-06 |
| `core-metadata` | 内嵌标签、指纹、外部来源接入、匹配裁决 | FR-07 |
| `core-share` | 分享模型、对端浏览接口 | FR-11 |
| `core-config` | 配置模型与校验 | FR-09 |
| `core-ipc` | 控制通道协议 | FR-08 |
| `core-log` | 结构化日志与隐私约束 | FR-09, 7.6 |
| `app-daemon` | Linux 无头服务可执行体 | FR-08 |
| `app-desktop` | Tauri 桌面可执行体 | FR-03/FR-09 等 |
| `app-android` | Android 壳（Kotlin + JNI） | 平台适配 |
