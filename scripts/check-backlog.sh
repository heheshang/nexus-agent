#!/usr/bin/env bash
# 校验 backlog 的组织纪律。
#
# 检查项：
#   1. 同一任务不得同时出现在 backlog/tasks/ 与 backlog/completed/
#      —— 归档用 `git mv`，但编辑器可能在旧路径重建同名文件（见 NEX-7 的教训）
#   2. backlog/completed/ 下的任务文件 front-matter 必须标记为已完成
#
# 用法：bash scripts/check-backlog.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

echo "── 1. 检查任务是否同时存在于 tasks/ 与 completed/ ──"
dupes=0
if [ -d backlog/tasks ] && [ -d backlog/completed ]; then
    for f in backlog/tasks/*.md; do
        [ -e "$f" ] || continue
        name="$(basename "$f")"
        if [ -e "backlog/completed/$name" ]; then
            echo "  ✗ 重复：$name 同时存在于 tasks/ 与 completed/"
            echo "      归档用 git mv；若被误留，请 git rm 掉 tasks/ 下的那一份"
            dupes=$((dupes + 1))
        fi
    done
fi
if [ "$dupes" -eq 0 ]; then
    echo "  ✓ 没有重复的任务文件"
else
    echo "  ✗ 发现 $dupes 个重复的任务文件"
    fail=1
fi

echo ""
echo "── 2. 检查 completed/ 下的任务是否标记为已完成 ──"
unmarked=0
for f in backlog/completed/*.md; do
    [ -e "$f" ] || continue
    if ! grep -qE '^status:[[:space:]]*(Done|Completed|已归档)' "$f"; then
        echo "  ✗ $(basename "$f") 的 status 未标记完成"
        unmarked=$((unmarked + 1))
    fi
done
if [ "$unmarked" -eq 0 ]; then
    echo "  ✓ 已归档任务均标记为完成"
else
    echo "  ✗ 发现 $unmarked 个未标记完成的任务文件"
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 全部检查通过"
else
    echo "✗ 存在检查失败项"
fi
exit "$fail"
