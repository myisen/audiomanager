# AudioSync RPM 打包说明
#
# 本目录包含 Fedora/RHEL/openSUSE 系发行版的 RPM 打包配置。
# spec 采用"预编译二进制重新布局"模式：cargo 先在本机构建，
# 再由 rpmbuild 将产物安装到标准 FHS 路径。
#
# 构建：
#   cargo build --release -p app-daemon -p app-desktop
#   ./scripts/build-rpm.sh
#
# 产物：release/rpm/audiosync-<version>-1.x86_64.rpm

## 安装路径布局
- /usr/bin/audiosync           — daemon + CLI
- /usr/bin/audiosync-desktop   — Tauri GUI（前端资源已内嵌）
- /usr/lib/systemd/user/audiosync.service — 用户级守护进程服务
- /usr/share/applications/audiosync.desktop — 桌面入口
- /usr/share/icons/hicolor/256x256/apps/audiosync.png
- /usr/share/licenses/audiosync/LICENSE

## 运行时依赖（Requires）
- webkit2gtk4.1 / gtk3 — GUI（Tauri on Linux = WebKitGTK 4.1）
- alsa-lib — rodio/cpal 音频输出
- openssl-libs — 网络传输 TLS

仅安装 daemon（不装 GUI）时 webkit2gtk4.1 仍会被带入；
如需精简可后续拆分为 audiosync-daemon 子包。
