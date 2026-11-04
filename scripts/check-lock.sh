#!/usr/bin/env bash
# 校验 workspace 的依赖锁定纪律。
#
# 检查项：
#   1. 成员 crate 的 Cargo.toml 不得出现具体版本号（必须走 workspace.dependencies）
#   2. workspace 必须存在 Cargo.lock（锁定解析结果）
#   3. Cargo.lock 与 Cargo.toml 是否同步（--locked 构建是否通过）
#
# 用法：bash scripts/check-lock.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

echo "── 1. 检查成员 crate 是否硬编码版本号 ──"
violations=0
for manifest in crates/*/Cargo.toml; do
    # 匹配形如 `serde = "1.0.229"` 或 `serde = { version = "..." }` 的写法
    if grep -nE '^[a-zA-Z0-9_-]+[[:space:]]*=[[:space:]]*"?[0-9]' "$manifest" \
       | grep -v '\.workspace' \
       | grep -v '^.*#' > /dev/null 2>&1; then
        echo "  ✗ $manifest"
        grep -nE '^[a-zA-Z0-9_-]+[[:space:]]*=[[:space:]]*"?[0-9]' "$manifest" \
            | grep -v '\.workspace' | sed 's/^/      /'
        violations=$((violations + 1))
    fi
done
if [ "$violations" -eq 0 ]; then
    echo "  ✓ 所有成员 crate 均通过 workspace.dependencies 引用依赖"
else
    echo "  ✗ 发现 $violations 个文件存在硬编码版本号"
    fail=1
fi

echo ""
echo "── 2. 检查 Cargo.lock 是否存在 ──"
if [ -f Cargo.lock ]; then
    locked=$(grep -c '^\[\[package\]\]' Cargo.lock)
    echo "  ✓ Cargo.lock 存在，已锁定 $locked 个 package"
else
    echo "  ✗ Cargo.lock 缺失"
    fail=1
fi

echo ""
echo "── 3. 检查 Cargo.lock 是否与 Cargo.toml 同步 ──"
if cargo metadata --locked --format-version 1 > /dev/null 2>&1; then
    echo "  ✓ Cargo.lock 与 manifest 同步（--locked 通过）"
else
    echo "  ✗ Cargo.lock 已过期，执行 'cargo update --workspace' 后重试"
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 全部检查通过"
else
    echo "✗ 存在检查失败项"
fi
exit "$fail"
