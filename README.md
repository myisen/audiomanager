# AudioSync

本地音乐互传播放器。将个人音频库作为多个「清单」管理，支持自然排序、
续播记忆、局域网设备配对与 P2P 文件互传、元数据自动同步（MusicBrainz /
Last.fm / ListenBrainz），三端可用：Linux 桌面与无头服务、Windows、Android。

## 功能特性

| 需求 | 说明 |
|---|---|
| FR-01 清单管理 | 目录即清单，白名单扩展名扫描，失效目录隔离标记 |
| FR-02 自然排序 | `1_ < 2_ < 10_ < album`，可手动锁定顺序 |
| FR-03 播放控制 | Symphonia 解码（mp3/flac/ogg/m4a/aac/wav），不可播放入库标记 |
| FR-04 续播 | 按文件记录位置，剩余 <5 秒视为播完自动重置 |
| FR-05/06 设备与传输 | 局域网发现/配对，TCP 直连 + coturn 中继回退，SHA-256 校验，断点续传，离线排队 |
| FR-07 元数据 | 内嵌标签 → 指纹(fpcalc) → MusicBrainz → Last.fm/ListenBrainz，冲突裁决 |
| FR-08 Linux 无头 | 单实例 daemon + Unix Socket IPC + systemd 自启，无音频设备降级 |
| FR-09 设置与隐私 | 全量设置项；离线降级、上行最小化、写回标签默认关闭 |
| FR-10 许可合规 | MIT；LGPL 组件子进程隔离，外部组件独立部署 |
| FR-11 分享 | 清单级/文件级两级分享开关，变更联动传输任务 |

## 仓库结构

```
crates/
├── core-store      持久化底座（JSON 存储）
├── core-config     配置模型与校验
├── core-playlist   清单模型与自然排序
├── core-library    目录扫描与清单维护
├── core-player     播放状态机与续播裁决
├── core-position   续播位置记录
├── core-cast       设备发现/配对/状态机
├── core-transfer   P2P 传输/断点续传/排队
├── core-share      分享规则与变更联动
├── core-metadata   标签/指纹/在线源/冲突裁决
├── core-log        结构化日志
├── core-platform   平台适配（音频/路径/自启动/防火墙）
├── core-ipc        JSON IPC 协议（三端共用）
├── core-jni        Android JNI 绑定（cdylib）
├── app-daemon      Linux 守护进程（CLI + Unix Socket）
└── app-desktop     Tauri 桌面应用
app-android/        Android 壳（Kotlin + SAF + 前台服务）
docs/               PRD、开发计划、架构、合规
scripts/            构建与许可清单脚本
```

分层规则见 [docs/architecture.md](docs/architecture.md)。

## 构建

```bash
# 全量检查（fmt/clippy/test）
cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test

# Linux 发行包（二进制 + systemd + 前端资源 + 合规文件）
./scripts/build-release.sh linux

# Windows（交叉编译 + WiX MSI）
./scripts/build-windows.sh

# Android（cargo-ndk + Gradle）
./scripts/build-android.sh
```

桌面 GUI 需要 Tauri Linux 系统依赖（libwebkit2gtk-4.1-dev 等）。

## 许可

本项目以 [MIT](LICENSE) 协议发布。第三方依赖许可清单见
[docs/third-party-licenses.md](docs/third-party-licenses.md)，
外部组件（Chromaprint/coturn）隔离方式见
[docs/compliance.md](docs/compliance.md)。
