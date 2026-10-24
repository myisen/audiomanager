# AudioSync RPM spec（Fedora/RHEL 系）
# 预编译二进制重新布局打包：二进制由 cargo 在本机构建后打进 Source0，
# rpmbuild 仅负责安装到 FHS 标准路径。
#
# 本地构建（不依赖 rpmbuild 联网）：
#   rpmbuild -bb --define "_topdir <topdir>" audiosync.spec

Name:           audiosync
Version:        0.1.0
Release:        1%{?dist}
Summary:        本地音乐互传播放器（守护进程 + 桌面端）

License:        MIT
URL:            https://github.com/myisen/audiomanager
Source0:        %{name}-%{version}-linux-x86_64.tar.gz

ExclusiveArch:  x86_64

# Ubuntu/Debian 的 rpm 无此宏；Fedora 上由系统定义
%{!?_userunitdir:%global _userunitdir /usr/lib/systemd/user}

Requires:       gtk3
Requires:       alsa-lib
Requires:       openssl-libs
Recommends:     webkit2gtk4.1
Recommends:     hicolor-icon-theme

%description
AudioSync 是局域网本地音乐互传播放器：
- audiosync          守护进程与 CLI（systemd 用户服务可选）
- audiosync-desktop  基于 Tauri/WebKitGTK 的桌面界面

守护进程监听本地控制通道（Unix socket），并在局域网内发现同类设备，
通过 NAT-free 的直连传输互相同步音乐清单与播放进度。

%prep
%setup -q

%install
install -Dpm 755 audiosync-daemon      %{buildroot}%{_bindir}/audiosync
install -Dpm 755 audiosync-desktop     %{buildroot}%{_bindir}/audiosync-desktop
install -Dpm 644 systemd/audiosync.service \
    %{buildroot}%{_userunitdir}/audiosync.service
install -Dpm 644 audiosync.desktop \
    %{buildroot}%{_datadir}/applications/audiosync.desktop
install -Dpm 644 audiosync.png \
    %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/%{name}.png

%files
%license LICENSE
%doc docs/third-party-licenses.md
%{_bindir}/audiosync
%{_bindir}/audiosync-desktop
%{_userunitdir}/audiosync.service
%{_datadir}/applications/audiosync.desktop
%{_datadir}/icons/hicolor/256x256/apps/%{name}.png

%changelog
* Tue Oct 13 2026 AudioSync Project <myisen@users.noreply.github.com> - 0.1.0-1
- 首个 RPM 发行（v0.1.0：daemon + CLI + Tauri 桌面端 + systemd 用户服务）
- 修复 IPC shutdown 请求后守护进程不退出的问题
