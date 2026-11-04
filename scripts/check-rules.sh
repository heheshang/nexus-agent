#!/usr/bin/env bash
# 校验 .cursor/rules/ 下规则文件的一致性。
#
# 检查项：
#   1. 每个 .mdc 文件有合法的 frontmatter（description 存在、alwaysApply 是布尔或缺省、globs 是合法 JSON 数组）
#   2. 规则中引用的**仓库路径**真实存在（scripts/、docs/、backlog/、crates/、configs/、examples/）
#   3. AGENTS.md 的规则清单与 .cursor/rules/ 下的实际文件一一对应（无遗漏、无幽灵条目）
#   4. 每个规则文件的「相关文件」段落引用的规则文件存在
#
# 为什么需要它：规则文件是**唯一的过程载体** ——
# 规则里引用一个不存在的脚本，等于给出一条无法执行的判据，
# 而「无法执行的判据」比「没有判据」更危险：它看起来是覆盖了的。
#
# 用法：bash scripts/check-rules.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RULES_DIR=".cursor/rules"
fail=0

# ─────────────────────────────────────────────────────────────
echo "── 1. 检查规则文件 frontmatter ──"
fm_bad=0
for f in "$RULES_DIR"/*.mdc; do
    [ -e "$f" ] || continue
    name="$(basename "$f")"

    if ! head -n 1 "$f" | grep -q '^---$'; then
        echo "  ✗ ${name}：首行不是 '---'，缺少 frontmatter"
        fm_bad=$((fm_bad + 1)); continue
    fi

    if ! sed -n '2,/^---$/p' "$f" | grep -qE '^description:[[:space:]]*\S'; then
        echo "  ✗ ${name}：frontmatter 缺 description"
        fm_bad=$((fm_bad + 1)); continue
    fi

    ap="$(sed -n '2,/^---$/p' "$f" | grep -E '^alwaysApply:' | head -n 1 || true)"
    if [ -n "$ap" ] && ! printf '%s' "$ap" | grep -qE ':[[:space:]]*(true|false)[[:space:]]*$'; then
        echo "  ✗ ${name}：alwaysApply 必须是 true 或 false"
        fm_bad=$((fm_bad + 1)); continue
    fi

    gl="$(sed -n '2,/^---$/p' "$f" | grep -E '^globs:' | head -n 1 || true)"
    if [ -n "$gl" ]; then
        if ! printf '%s' "$gl" | sed 's/^globs:[[:space:]]*//' | python3 -c 'import json,sys; json.loads(sys.stdin.read())' 2>/dev/null; then
            echo "  ✗ ${name}：globs 不是合法 JSON 数组"
            fm_bad=$((fm_bad + 1)); continue
        fi
    fi
done
if [ "$fm_bad" -eq 0 ]; then
    echo "  ✓ 全部规则文件 frontmatter 合法"
else
    echo "  ✗ 发现 $fm_bad 个 frontmatter 问题"
    fail=1
fi

echo ""
# ─────────────────────────────────────────────────────────────
echo "── 2. 检查规则中引用的仓库路径是否存在 ──"
# 提取形如 `scripts/xxx.sh` 的反引号引用。
# 只检查本仓库内的一级目录前缀 —— 外部路径（~/.claude/...）不在管辖范围。
# 含占位符（< > * { }）的是**模板路径**，跳过。
path_bad_file="$(mktemp)"
for f in "$RULES_DIR"/*.mdc; do
    [ -e "$f" ] || continue
    name="$(basename "$f")"
    grep -oE '`(scripts|docs|backlog|crates|configs|examples)/[A-Za-z0-9._/-]+`' "$f" \
        | tr -d '`' | sort -u > "$path_bad_file.tmp" || true
    while read -r p; do
        [ -n "$p" ] || continue
        case "$p" in
            *'<'*|*'>'*|*'*'*|*'{'*|*'}'*) continue ;;  # 模板路径
        esac
        if [ ! -e "$p" ]; then
            echo "  ✗ ${name}：引用了不存在的路径 '$p'"
            echo "$name|$p" >> "$path_bad_file"
        fi
    done < "$path_bad_file.tmp"
done
rm -f "$path_bad_file.tmp"
path_bad="$(wc -l < "$path_bad_file" | tr -d ' ')"
rm -f "$path_bad_file"
if [ "$path_bad" -eq 0 ]; then
    echo "  ✓ 规则中引用的仓库路径全部存在"
else
    echo "  ✗ 发现 $path_bad 处无效路径引用"
    fail=1
fi

echo ""
# ─────────────────────────────────────────────────────────────
echo "── 3. 检查 AGENTS.md 规则清单与实际文件一一对应 ──"
list_bad=0
if [ ! -f AGENTS.md ]; then
    echo "  ✗ AGENTS.md 不存在"
    fail=1
else
    listed="$(grep -oE '\.cursor/rules/[A-Za-z0-9._-]+\.mdc' AGENTS.md | sed 's|.*/||' | sort -u)"
    actual="$(ls "$RULES_DIR" | sort -u)"

    while read -r rf; do
        [ -n "$rf" ] || continue
        if ! printf '%s\n' "$listed" | grep -qx "$rf"; then
            echo "  ✗ $rf 存在于 .cursor/rules/ 但未出现在 AGENTS.md 的规则清单中"
            list_bad=$((list_bad + 1))
        fi
    done <<< "$actual"

    while read -r rf; do
        [ -n "$rf" ] || continue
        if [ ! -e "$RULES_DIR/$rf" ]; then
            echo "  ✗ AGENTS.md 列出的 $rf 在 .cursor/rules/ 下不存在"
            list_bad=$((list_bad + 1))
        fi
    done <<< "$listed"

    if [ "$list_bad" -eq 0 ]; then
        echo "  ✓ AGENTS.md 规则清单与实际文件一一对应"
    else
        echo "  ✗ 发现 $list_bad 处清单漂移"
        fail=1
    fi
fi

echo ""
# ─────────────────────────────────────────────────────────────
echo "── 4. 检查「相关文件」段落引用的规则文件存在 ──"
rel_bad=0
for f in "$RULES_DIR"/*.mdc; do
    [ -e "$f" ] || continue
    name="$(basename "$f")"
    refs="$(awk '/^## 相关文件/{flag=1;next} flag' "$f" \
        | grep -oE '`[A-Za-z0-9._-]+\.mdc`' | tr -d '`' | sort -u || true)"
    while read -r rf; do
        [ -n "$rf" ] || continue
        if [ ! -e "$RULES_DIR/$rf" ]; then
            echo "  ✗ $name 的「相关文件」引用了不存在的规则文件 $rf"
            rel_bad=$((rel_bad + 1))
        fi
    done <<< "$refs"
done
if [ "$rel_bad" -eq 0 ]; then
    echo "  ✓ 「相关文件」引用的规则文件全部存在"
else
    echo "  ✗ 发现 $rel_bad 处无效规则文件引用"
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 全部检查通过"
else
    echo "✗ 存在检查失败项"
fi
exit "$fail"
