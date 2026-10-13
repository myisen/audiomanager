# 合规结论（M11 · T111）

> 审查日期：2026-10-13
> 范围：全部 Rust 工作区依赖（471 条）、Android 端依赖、外部可选组件
> 结论：**许可清单完整且无冲突，MIT LICENSE 随发行包提供，满足 FR-10 与 M11 退出条件。**

## 1. 依赖许可清单

完整清单见 [third-party-licenses.md](third-party-licenses.md)（由
`scripts/gen-license-list.sh` 自动生成，来源 `cargo metadata`）。

### 许可分布（共 471 条）

| 许可 | 条目数 | 与 MIT 分发兼容性 |
|---|---|---|
| MIT OR Apache-2.0 | 224 | ✅ 双许可任选 |
| MIT | 128 | ✅ |
| Apache-2.0 | 39 | ✅ |
| Zlib | 19 | ✅ |
| Unicode-3.0 | 18 | ✅ |
| MPL-2.0 | 17 | ✅ 文件级弱 copyleft |
| BSD-3-Clause | 6 | ✅ |
| Apache-2.0 WITH LLVM-exception | 4 | ✅ |
| ISC / CC0-1.0 / 0BSD | 3 | ✅ |
| MIT OR Apache-2.0 OR LGPL-2.1-or-later | 2 | ✅ 见 §2 说明 |

无 UNKNOWN 许可；无 GPL / AGPL / SSPL 强传染许可进入依赖树。

### 特殊条目说明

- **`r-efi`（5.3.0 / 6.0.0）**：三重许可
  `MIT OR Apache-2.0 OR LGPL-2.1-or-later`。OR 许可下分发方可任选其一，
  我们选择 MIT 分支，LGPL 条款不触发。该 crate 仅在 UEFI 构建目标下被
  `windows` crate 引用，运行时不受影响。
- **`symphonia` 系列**：MPL-2.0（M2/T21 已确认）。文件级弱 copyleft，
  仅需保留该文件版权声明，与 MIT 分发兼容。

## 2. 外部组件隔离复核（FR-10）

### 2.1 Chromaprint（音频指纹，T62）

| 项目 | 结论 |
|---|---|
| 许可 | LGPL-2.1 |
| 隔离方式 | **fpcalc 独立命令行子进程**（[core-metadata `compute_fingerprint`](../crates/core-metadata/src/lib.rs)） |
| 链接 | 无静态/动态链接，仅经 stdin/stdout 交换文本 |
| 分发 | 不随发行包捆绑；用户自备安装（apt/choco/brew） |
| 失败行为 | 不可用时降级 `FingerprintStatus::Unsupported`，不影响其他功能 |

LGPL 传染性仅作用于链接产物；子进程调用不产生衍生作品，**合规**。

### 2.2 coturn（STUN/TURN 中继，T42/T44）

| 项目 | 结论 |
|---|---|
| 许可 | BSD-3-Clause |
| 隔离方式 | **独立部署服务器组件**，与客户端仅走 RFC 5766/8489 网络协议 |
| 链接 | 无任何代码链接 |
| 分发 | 不随发行包分发；自建/第三方 coturn 服务均可 |
| 依赖关系 | `core-transfer` 的中继回退仅将 coturn 地址作为传输端点配置 |

BSD-3-Clause 允许任意使用与分发，仅需保留声明；协议交互无传染，**合规**。

## 3. 各端运行时依赖

| 端 | 依赖 | 许可 |
|---|---|---|
| Rust 全端 | 见 third-party-licenses.md | 均在 deny.toml 白名单 |
| Linux 桌面 | GTK3 / WebKitGTK（系统库） | LGPL-2.1（系统库例外，仅动态链接，等效独立组件） |
| Windows | WiX Toolset（仅构建期） | MS-RL（不进入产物） |
| Android | androidx.core/appcompat/recyclerview/media/lifecycle | Apache-2.0 |
| Android | Kotlin 标准库 | Apache-2.0 |
| 桌面前端 | 手写 HTML/CSS/JS（dist/），无 npm 运行时依赖 | 本项目 MIT |

## 4. 发行包合规清单

每个发行包必须附带：

- [x] `LICENSE`（MIT，本仓库根目录）
- [x] `docs/third-party-licenses.md`（第三方许可清单）
- [x] 本文档（外部组件隔离说明）

由 `scripts/build-release.sh` 统一打包（T112）。
