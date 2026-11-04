#!/usr/bin/env bash
# 校验「标识符不得包含中文」的铁律。
#
# 规则（见 .cursor/rules/rust-code-standards.mdc §11）：
#   1. 函数名必须是 ASCII 标识符（Rust: snake_case；TS/JS: camelCase）
#   2. 中文只允许出现在注释、字符串字面量、文档字符串里
#
# 理由：
#   - Rust 允许非 ASCII 标识符（RFC 2457），但中文函数名在 rust-analyzer、
#     终端 diff、grep、CI 日志里都难以检索与引用
#   - TS/JS 的中文标识符会直接破坏 `npm run` 生态的大量工具链
#   - 中英混排的命名会让「注释解释为什么」的规则失去意义 —— 名字应是最稳定的契约
#
# 用法：bash scripts/check-naming.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

# Rust：函数定义 `fn <name>`（覆盖 async / unsafe / const / extern 前缀）
# TS/JS：函数定义 `function <name>` 与箭头函数 `const <name> = ...`
#
# 标识符的正则必须**同时**覆盖两种位置：
#   - 中文开头：`fn 校验_名称()`
#   - 中文在后：`fn ast_校验不保证语义合理()`
# 早期版本只匹配「中文开头」，导致 `sqlx_只出现在数据访问层` 这类
# 「英文前缀 + 中文描述」的写法长期漏检。
RUST_RE='(?:async[[:space:]]+|unsafe[[:space:]]+|const[[:space:]]+|extern[[:space:]]+"[^"]*"[[:space:]]+)*fn[[:space:]]+[^\s(<]*(?:\p{Han}[^\s(<]*)?[^\s(<]*\p{Han}[^\s(<]*'
TS_RE='(?:function[[:space:]]+|const[[:space:]]+|let[[:space:]]+|var[[:space:]]+|class[[:space:]]+)(?:\p{Han}[\p{L}\p{N}_$]*|[\p{L}\p{N}_$]*\p{Han}[\p{L}\p{N}_$]*)[[:space:]]*(?:=|\(|extends|implements|\{)'

echo "── 规则 1：Rust 函数名不得含中文 ──"
hits=$(rg -n --pcre2 "$RUST_RE" \
    -g '*.rs' \
    -g '!target/**' \
    -g '!**/target/**' \
    crates examples 2>/dev/null || true)
if [ -n "$hits" ]; then
    echo "$hits" | sed 's/^/  ✗ /'
    fail=1
else
    echo "  ✓ 无中文函数名"
fi

echo ""
echo "── 规则 2：TS/JS 函数名不得含中文 ──"
ts_hits=""
if [ -d frontend ]; then
    ts_hits=$(rg -n --pcre2 "$TS_RE" \
        -g '*.ts' -g '*.tsx' -g '*.js' -g '*.jsx' -g '*.mjs' -g '*.cjs' \
        -g '!node_modules/**' -g '!dist/**' \
        frontend 2>/dev/null || true)
fi
if [ -n "$ts_hits" ]; then
    echo "$ts_hits" | sed 's/^/  ✗ /'
    fail=1
else
    echo "  ✓ 无中文函数名"
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 标识符命名检查全部通过"
else
    echo "✗ 存在含中文的函数名"
fi
exit "$fail"
