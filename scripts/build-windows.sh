# AudioSync Windows 交叉编译脚本
# T93: 在 Linux 上交叉编译 Windows 发行物

set -euo pipefail

echo "=== AudioSync Windows 交叉编译 ==="

# 1. 安装 cross
if ! command -v cross &>/dev/null; then
    cargo install cross --git https://github.com/cross-rs/cross || true
fi

# 2. 编译 Windows x86_64
echo ">>> 编译 audiosync-desktop (Windows x86_64)"
cross build --release --target x86_64-pc-windows-gnu -p app-desktop

echo ">>> 编译 audiosync daemon (Windows x86_64)"
cross build --release --target x86_64-pc-windows-gnu -p app-daemon

# 3. 复制产物
BUILD_DIR="target/x86_64-pc-windows-gnu/release"
DIST_DIR="dist/windows"

mkdir -p "$DIST_DIR/assets"

cp "$BUILD_DIR/audiosync-desktop.exe" "$DIST_DIR/"
# 构建产物名为 audiosync.exe，打包时重命名以区分桌面端
cp "$BUILD_DIR/audiosync.exe" "$DIST_DIR/audiosync-daemon.exe"
# WebView2Loader.dll：windows-gnu target 不静态链接 WebView2 加载器，
# 必须随 exe 分发（版本须与 Cargo.lock 中 webview2-com-sys 一致）
cp crates/app-desktop/packaging/WebView2Loader.dll "$DIST_DIR/"
cp -r dist/index.html dist/style.css dist/app.js "$DIST_DIR/assets/"
cp crates/app-desktop/packaging/add-firewall-rules.ps1 "$DIST_DIR/"
cp crates/app-desktop/icons/icon.ico "$DIST_DIR/"
cp LICENSE "$DIST_DIR/"

echo ">>> Windows 发行物已构建到 $DIST_DIR/"
ls -la "$DIST_DIR/"
