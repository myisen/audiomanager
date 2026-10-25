#!/usr/bin/env bash
# AudioSync RPM 打包（Fedora/RHEL/openSUSE 系）
# 用法：./scripts/build-rpm.sh
# 前置：cargo build --release -p app-daemon -p app-desktop，且已安装 rpmbuild
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep -m1 '^version' Cargo.toml | tr -d ' "version=' || echo "0.1.0")"
VERSION="${VERSION:-0.1.0}"
TOP="${RPM_TOPDIR:-/tmp/audiosync-rpmbuild}"
DIST_TAG="${RPM_DIST:-}"

[ -x target/release/audiosync ] || { echo "缺少 target/release/audiosync，先执行 cargo build --release -p app-daemon -p app-desktop"; exit 1; }
command -v rpmbuild >/dev/null || { echo "未安装 rpmbuild"; exit 1; }

rm -rf "$TOP" && mkdir -p "$TOP"/{SOURCES,SPECS,RPMS,SRPMS,BUILD}
STAGE="$TOP/staging/audiosync-$VERSION"
mkdir -p "$STAGE/systemd" "$STAGE/docs"

cp target/release/audiosync "$STAGE/audiosync-daemon"
cp target/release/audiosync-desktop "$STAGE/"
cp crates/app-daemon/systemd/audiosync.service "$STAGE/systemd/"
cp crates/app-desktop/packaging/rpm/audiosync.desktop "$STAGE/"
cp crates/app-desktop/icons/256x256.png "$STAGE/audiosync.png"
cp LICENSE "$STAGE/"
cp docs/third-party-licenses.md "$STAGE/docs/"
cp docs/rpm-guide.md "$STAGE/docs/"

tar czf "$TOP/SOURCES/audiosync-$VERSION-linux-x86_64.tar.gz" -C "$TOP/staging" "audiosync-$VERSION"
cp crates/app-desktop/packaging/rpm/audiosync.spec "$TOP/SPECS/"

rpmbuild -bb --define "_topdir $TOP" ${DIST_TAG:+--define "dist $DIST_TAG"} "$TOP/SPECS/audiosync.spec"

mkdir -p release/rpm
find "$TOP/RPMS" -name "*.rpm" -exec cp {} release/rpm/ \;
echo ">>> RPM 已生成:"
ls -lh release/rpm/
