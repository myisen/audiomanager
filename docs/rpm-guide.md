# AudioSync RPM 安装与卸载指南

适用于 Fedora 40+ / RHEL 10 / openSUSE Tumbleweed（x86_64）等 RPM 系发行版。
安装包：`audiosync-0.1.0-1.fc41.x86_64.rpm`

## 1. 系统要求

| 项 | 要求 |
|---|---|
| 架构 | x86_64 |
| glibc | ≥ 2.39（Fedora 40 / RHEL 10 及以上） |
| GUI 依赖 | `webkit2gtk4.1`、`gtk3`（仅桌面端需要，dnf 会自动安装） |
| 音频 | `alsa-lib`（ALSA 音频输出） |
| systemd | 可选，用于用户级守护进程服务 |

## 2. 安装

### 推荐：dnf 自动处理依赖

```bash
sudo dnf install ./audiosync-0.1.0-1.fc41.x86_64.rpm
```

dnf 会自动安装 webkit2gtk4.1、gtk3、alsa-lib 等运行时依赖。

### 备选方式

```bash
# yum（RHEL 系老版本）
sudo yum localinstall ./audiosync-0.1.0-1.fc41.x86_64.rpm

# rpm（不自动解决依赖，不推荐）
sudo rpm -ivh audiosync-0.1.0-1.fc41.x86_64.rpm
```

### 安装内容

```
/usr/bin/audiosync                            守护进程 + CLI
/usr/bin/audiosync-desktop                    桌面界面（Tauri/WebKitGTK）
/usr/lib/systemd/user/audiosync.service       用户级 systemd 服务
/usr/share/applications/audiosync.desktop     桌面菜单入口
/usr/share/icons/hicolor/256x256/apps/audiosync.png
/usr/share/licenses/audiosync-0.1.0/LICENSE
/usr/share/doc/audiosync-0.1.0/third-party-licenses.md
```

### 安装后验证

```bash
rpm -q audiosync          # 确认包版本
audiosync help            # CLI 可用
```

## 3. 启动使用

### 守护进程（二选一）

**systemd 用户服务（推荐，开机自启 + 崩溃自动重启）：**

```bash
systemctl --user enable --now audiosync.service   # 启动并设为自启
systemctl --user status audiosync.service         # 查看状态
journalctl --user -u audiosync.service -f         # 查看日志
```

> 若提示 `Failed to connect to bus`，先执行 `sudo loginctl enable-linger $USER`。

**手动启动（调试用）：**

```bash
audiosync daemon
```

### 常用命令

```bash
audiosync select ~/Music   # 选择音乐目录（清单）
audiosync status           # 播放状态
audiosync list             # 清单列表
audiosync play             # 播放/暂停
audiosync next / prev      # 下一首 / 上一首
audiosync volume 60        # 音量 0-100
audiosync seek 30          # 跳转到第 30 秒
audiosync sync             # 同步元数据
audiosync transfers        # 传输记录
audiosync shutdown         # 停止守护进程
```

### 桌面端

应用菜单中点击 **AudioSync**，或命令行运行 `audiosync-desktop`。

## 4. 升级

```bash
sudo dnf upgrade ./audiosync-<新版本>-1.fc41.x86_64.rpm
# 或
sudo dnf install ./audiosync-<新版本>-1.fc41.x86_64.rpm   # dnf 自动覆盖旧版
```

升级前建议停止服务：

```bash
systemctl --user stop audiosync.service
```

## 5. 卸载

```bash
# 1. 停止并取消守护进程服务（若已启用）
systemctl --user disable --now audiosync.service

# 2. 卸载软件包
sudo dnf remove audiosync
# 或：sudo rpm -e audiosync
```

卸载会移除上节"安装内容"列出的全部文件（二进制、服务、桌面入口、图标）。

### 用户数据清理（可选）

RPM 卸载**不会删除**以下用户数据，彻底清除请手动删除：

```bash
rm -rf ~/.local/share/audiosync     # 音乐清单与数据库（$XDG_DATA_HOME/audiosync）
rm -rf ~/.config/audiosync          # 配置文件（$XDG_CONFIG_HOME/audiosync）
rm -rf "$XDG_RUNTIME_DIR/audiosync" # 运行时 socket（$XDG_RUNTIME_DIR/audiosync）
```

> 重新安装后这些数据仍会被识别，仅在不打算再使用时删除。

## 6. 故障排查

| 现象 | 原因与处理 |
|---|---|
| `dlopen: libwebkit2gtk-4.1.so.0: cannot open` | 缺 GUI 依赖：`sudo dnf install webkit2gtk4.1` |
| `/usr/bin/audiosync: version 'GLIBC_2.39' not found` | 系统 glibc 过旧（< Fedora 40），请升级系统或改用静态构建 |
| 桌面图标不显示 | 重新登录会话刷新图标缓存，或 `sudo update-desktop-database` |
| `audiosync status` 报连接失败 | 守护进程未启动：`systemctl --user start audiosync.service` |
| 音频无输出 | 检查 ALSA：`sudo dnf install alsa-lib` 并确认 `aplay -l` 能列出设备 |

## 7. 包信息

- 许可证：MIT（含第三方许可清单，见 `/usr/share/doc/audiosync-0.1.0/`）
- 项目主页：<https://github.com/myisen/audiomanager>
- 问题反馈：<https://github.com/myisen/audiomanager/issues>
