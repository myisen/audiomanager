#!/usr/bin/env bash
# 生成第三方依赖许可清单（FR-10 §3）
# 用法：./scripts/gen-license-list.sh [输出路径]
# 默认输出到 docs/third-party-licenses.md
set -euo pipefail

OUTPUT="${1:-docs/third-party-licenses.md}"

# 检查 cargo-deny 是否可用
if ! command -v cargo-deny &> /dev/null; then
    echo "错误：cargo-deny 未安装。请运行: cargo install cargo-deny --locked"
    exit 1
fi

cd "$(dirname "$0")/.."

echo "# 第三方依赖许可清单" > "$OUTPUT"
echo "" >> "$OUTPUT"
echo "> 本文件由 \`scripts/gen-license-list.sh\` 自动生成，请勿手动编辑。" >> "$OUTPUT"
echo "> 生成时间：$(date -u +"%Y-%m-%d %H:%M:%S UTC")" >> "$OUTPUT"
echo "" >> "$OUTPUT"

# 用 cargo deny list 输出 JSON，再转换为 Markdown 表格
cargo deny list --format json 2>/dev/null \
    | python3 -c '
import sys, json

data = json.load(sys.stdin)
# data.licenses = [["MIT", ["name 0.1.0 source", ...]], ...]
rows = []
for lic, crates in data.get("licenses", []):
    for spec in crates:
        # spec 格式: "name version source"
        parts = spec.split(" ", 2)
        if len(parts) >= 2:
            name, ver = parts[0], parts[1]
            # 过滤掉工作区内部 crate（path 依赖），仅保留第三方依赖
            if "path+file" not in spec:
                rows.append((name, ver, lic))

rows.sort(key=lambda r: r[0])
print("| Crate | Version | License |")
print("|---|---|---|")
for name, ver, lic in rows:
    print(f"| {name} | {ver} | {lic} |")
' >> "$OUTPUT"

echo "" >> "$OUTPUT"
echo "---" >> "$OUTPUT"
echo "" >> "$OUTPUT"
echo "## 许可策略" >> "$OUTPUT"
echo "" >> "$OUTPUT"
echo "- 本产品代码以 MIT 协议发布" >> "$OUTPUT"
echo "- 所有第三方依赖的许可必须与 MIT 分发兼容（见 deny.toml）" >> "$OUTPUT"
echo "- GPL / AGPL 等强传染性许可不允许进入主进程" >> "$OUTPUT"
echo "- LGPL 依赖需通过独立进程隔离，不静态链接" >> "$OUTPUT"

echo "依赖许可清单已生成: $OUTPUT"
