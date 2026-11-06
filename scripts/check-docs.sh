#!/usr/bin/env bash
# 校验 docs/ 下的文档一致性。
#
# 检查项：
#   1. docs/ 内的相对链接可解析（指向的文件真实存在）
#   2. docs/README.md 的索引覆盖 docs/ 下的全部 .md 文件（无遗漏、无幽灵条目）
#   3. docs/ai-sdlc/coverage-matrix.md 中「本仓库载体」列引用的仓库路径真实存在
#      （覆盖矩阵已于 2026-11-05 移除 —— 存在即校验，不存在即跳过）
#
# 为什么需要它：**指向具体产物的引用必须被机器校验存在性。**
# 规则文件引用了不存在的脚本，会给出一条无法执行的判据 ——
# 而「无法执行的判据」比「没有判据」更危险：它看起来是覆盖了的。
# 文档同理：索引里列了一个不存在的文档，读者会以为读过它了。
#
# 用法：bash scripts/check-docs.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DOCS_DIR="docs"
INDEX="$DOCS_DIR/README.md"
MATRIX="$DOCS_DIR/ai-sdlc/coverage-matrix.md"
fail=0

# ─────────────────────────────────────────────────────────────
echo "── 1. 检查 docs/ 内的相对链接 ──"
# 提取形如 [text](path) 的链接。跳过：
#   - 以 http(s):// 开头的外部链接
#   - 以 # 开头的锚点
#   - 含 < > 的模板路径
#   - 含空格且以 / 结尾的目录链接（目录允许，但需存在）
link_bad_file="$(mktemp)"
link_total=0
while IFS= read -r md; do
    rel="${md#"$DOCS_DIR"/}"
    dir="$(dirname "$md")"
    # 只取行内代码块之外的链接 —— 提取所有 ](...) 即可，代码块里的示例链接
    # 指向的也是本仓库路径，一并校验更严格。
    grep -oE '\]\([^)]+\)' "$md" 2>/dev/null | sed 's/^](//; s/)$//' \
        | sed 's/[[:space:]]*$//' > "$link_bad_file.links" || true
    while IFS= read -r link; do
        [ -n "$link" ] || continue
        case "$link" in
            http://*|https://*|mailto:*|\\*) continue ;;
            \#*) continue ;;
            *'<'*|*'>'*) continue ;;
        esac
        # 去掉锚点与查询串
        target="${link%%#*}"
        target="${target%%\?*}"
        [ -n "$target" ] || continue
        link_total=$((link_total + 1))
        if [ ! -e "$dir/$target" ]; then
            echo "  ✗ ${rel}：链接指向不存在的路径 '$target'"
            echo "$rel|$target" >> "$link_bad_file"
        fi
    done < "$link_bad_file.links"
done < <(find "$DOCS_DIR" -name '*.md' -type f | sort)
rm -f "$link_bad_file.links"
link_bad="$(wc -l < "$link_bad_file" | tr -d ' ')"
rm -f "$link_bad_file"
if [ "$link_bad" -eq 0 ]; then
    echo "  ✓ ${link_total} 个相对链接全部可解析"
else
    echo "  ✗ 发现 $link_bad 处无效链接"
    fail=1
fi

echo ""
# ─────────────────────────────────────────────────────────────
echo "── 2. 检查 docs/README.md 索引覆盖全部文档 ──"
index_bad=0
if [ ! -f "$INDEX" ]; then
    echo "  ✗ $INDEX 不存在"
    fail=1
else
    # 索引里出现过的目标路径（去掉锚点）
    grep -oE '\]\([^)]+\)' "$INDEX" \
        | sed 's/^](//; s/)$//' \
        | grep -vE '^(https?://|mailto:|#)' \
        | sed 's/#.*//' \
        | sed 's|[[:space:]]*$||' \
        | sort -u > /tmp/.docs_index_list.$$

    while IFS= read -r md; do
        rel="${md#"$DOCS_DIR"/}"
        # 索引本身不需要出现在索引里
        [ "$rel" = "README.md" ] && continue
        if ! grep -qxF "$rel" /tmp/.docs_index_list.$$; then
            echo "  ✗ docs/$rel 未出现在 docs/README.md 的索引中"
            index_bad=$((index_bad + 1))
        fi
    done < <(find "$DOCS_DIR" -name '*.md' -type f | sort)

    # 幽灵条目：索引里列了但文件不存在
    while IFS= read -r rel; do
        [ -n "$rel" ] || continue
        if [ ! -e "$DOCS_DIR/$rel" ]; then
            echo "  ✗ docs/README.md 索引列出了不存在的文档 '$rel'"
            index_bad=$((index_bad + 1))
        fi
    done < /tmp/.docs_index_list.$$
    rm -f /tmp/.docs_index_list.$$

    if [ "$index_bad" -eq 0 ]; then
        echo "  ✓ docs/README.md 索引与实际文档一一对应"
    else
        echo "  ✗ 发现 $index_bad 处索引漂移"
        fail=1
    fi
fi

echo ""
# ─────────────────────────────────────────────────────────────
echo "── 3. 检查覆盖矩阵引用的仓库路径存在 ──"
# ⚠️ 覆盖矩阵是**可选**的 —— `docs/ai-sdlc/` 目录已于 2026-11-05
# 被移除（那批文档是「上游规范适配」的说明材料，不是本仓库的**产出物**）。
#
# 所以：矩阵**存在**就校验它，**不存在**就跳过 ——
# 但**不能**把「矩阵不存在」当成**失败**。
#
# 为什么不能简单地**删掉**这条规则：
# 若日后重新引入覆盖矩阵，这条规则应当**自动生效** ——
# 留着它并让它「存在即校验」比「删掉后手工补回」更稳。
matrix_bad_file="$(mktemp)"
matrix_total=0
if [ ! -f "$MATRIX" ]; then
    echo "  – 覆盖矩阵不存在（已移除），跳过该检查"
else
    # 只取「本仓库载体」相关的引用：反引号包裹的仓库路径
    grep -oE '`(scripts|docs|backlog|crates|configs|examples|\.cursor)/[A-Za-z0-9._/-]+`' "$MATRIX" \
        | tr -d '`' | sort -u > "$matrix_bad_file.tmp" || true
    while IFS= read -r p; do
        [ -n "$p" ] || continue
        case "$p" in
            *'<'*|*'>'*|*'*'*|*'{'*|*'}'*) continue ;;  # 模板路径
        esac
        matrix_total=$((matrix_total + 1))
        if [ ! -e "$p" ]; then
            echo "  ✗ coverage-matrix.md：引用了不存在的路径 '$p'"
            echo "$p" >> "$matrix_bad_file"
        fi
    done < "$matrix_bad_file.tmp"
    rm -f "$matrix_bad_file.tmp"
fi
matrix_bad="$(wc -l < "$matrix_bad_file" | tr -d ' ')"
rm -f "$matrix_bad_file"
if [ "$matrix_bad" -eq 0 ]; then
    echo "  ✓ 覆盖矩阵引用的 ${matrix_total} 个仓库路径全部存在"
else
    echo "  ✗ 发现 $matrix_bad 处无效路径引用"
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 全部检查通过"
else
    echo "✗ 存在检查失败项"
fi
exit "$fail"
