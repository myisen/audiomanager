#!/usr/bin/env bash
# 生成第三方依赖许可清单（FR-10 §3）
# 用法：./scripts/gen-license-list.sh [输出路径]
# 默认输出到 docs/third-party-licenses.md
#
# 数据来源：cargo metadata（读取本地 registry 缓存的许可元数据），
# 不依赖 cargo-deny。许可准入策略由 deny.toml 定义。
set -euo pipefail

OUTPUT="${1:-docs/third-party-licenses.md}"

cd "$(dirname "$0")/.."

python3 - <<'PY' > "$OUTPUT"
import json, subprocess, datetime

meta = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--locked"],
    capture_output=True, text=True, check=True,
).stdout)

workspace_members = set(meta["workspace_members"])
rows = {}
for pkg in meta["packages"]:
    if pkg["id"] in workspace_members:
        continue  # 工作区内部 crate
    lic = pkg.get("license") or pkg.get("license_file") or "UNKNOWN"
    key = (pkg["name"], pkg["version"])
    rows.setdefault(key, set()).add(lic)

print("# 第三方依赖许可清单")
print()
print("> 本文件由 `scripts/gen-license-list.sh` 自动生成，请勿手动编辑。")
print(f"> 生成时间：{datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%d %H:%M:%S')} UTC")
print()
print("| Crate | Version | License |")
print("|---|---|---|")
for (name, ver), lics in sorted(rows.items()):
    print(f"| {name} | {ver} | {' OR '.join(sorted(lics))} |")
print()
print("---")
print()
print("## 许可策略")
print()
print("- 本产品代码以 MIT 协议发布")
print("- 所有第三方依赖的许可必须与 MIT 分发兼容（见 deny.toml）")
print("- GPL / AGPL 等强传染性许可不允许进入主进程")
print("- LGPL 依赖需通过独立进程隔离，不静态链接（Chromaprint → fpcalc 子进程）")
print("- 外部可选组件（coturn 等）独立部署，不随发行包分发，见 docs/compliance.md")
PY

echo "依赖许可清单已生成: $OUTPUT"
