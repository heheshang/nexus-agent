#!/usr/bin/env bash
# 校验 crate 依赖方向的「铁律」。
#
# 规则（见 docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §0.3）：
#   1. nexus-core 不依赖任何内部 crate（最底层）
#   2. 能力层（llm/tool/rag/memory/capacity/execution/workflow/...）不得依赖
#      service / api / cli —— 否则会形成环
#   3. nexus-repo 是唯一允许直接接触 SQLx 的数据访问层（infra 除外）
#   4. nexus-infra 不依赖任何上层 crate
#
# 用法：bash scripts/check-deps.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

# crate -> 内部依赖列表（与 scaffold.sh 保持同步）
deps_of() {
    case "$1" in
        nexus-core)          echo "" ;;
        nexus-config)        echo "nexus-core" ;;
        nexus-model)         echo "nexus-core" ;;
        nexus-observability) echo "nexus-core" ;;
        nexus-infra)         echo "nexus-core nexus-config nexus-observability" ;;
        nexus-security)      echo "nexus-core nexus-config nexus-observability" ;;
        nexus-vector)        echo "nexus-core nexus-config nexus-infra nexus-observability" ;;
        nexus-llm)           echo "nexus-core nexus-config nexus-model nexus-observability nexus-security" ;;
        nexus-tool)          echo "nexus-core nexus-config nexus-model nexus-llm nexus-observability nexus-security" ;;
        nexus-rag)           echo "nexus-core nexus-config nexus-model nexus-llm nexus-observability nexus-security nexus-vector" ;;
        nexus-ingestion)     echo "nexus-core nexus-config nexus-model nexus-infra nexus-observability" ;;
        nexus-capacity)      echo "nexus-core nexus-config nexus-model nexus-observability" ;;
        nexus-workflow)      echo "nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-rag nexus-observability" ;;
        nexus-execution)     echo "nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-workflow nexus-capacity nexus-infra nexus-observability nexus-security nexus-repo" ;;
        nexus-repo)          echo "nexus-core nexus-config nexus-model nexus-infra nexus-observability nexus-security nexus-capacity nexus-rag nexus-ingestion nexus-vector" ;;
        nexus-agent)         echo "nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-observability nexus-security" ;;
        nexus-memory)        echo "nexus-core nexus-config nexus-model nexus-llm nexus-observability nexus-security nexus-vector nexus-ingestion" ;;
        nexus-mcp)           echo "nexus-core nexus-config nexus-model nexus-security nexus-observability" ;;
        nexus-a2a)           echo "nexus-core nexus-config nexus-model nexus-security nexus-observability nexus-mcp" ;;
        nexus-service)       echo "nexus-core nexus-config nexus-model nexus-observability nexus-infra nexus-security nexus-llm nexus-tool nexus-rag nexus-memory nexus-capacity nexus-workflow nexus-execution nexus-repo nexus-agent nexus-ingestion nexus-mcp nexus-a2a" ;;
        nexus-api)           echo "nexus-core nexus-config nexus-model nexus-observability nexus-security nexus-service nexus-agent nexus-execution nexus-llm" ;;
        nexus-cli)           echo "nexus-core nexus-config nexus-observability nexus-service nexus-api" ;;
        *)                   echo "" ;;
    esac
}

ALL_CRATES=$(ls -1 crates | sed 's/^/nexus-/;s/^nexus-nexus-/nexus-/')

echo "── 规则 1：nexus-core 不依赖任何内部 crate ──"
if [ -z "$(deps_of nexus-core)" ]; then
    echo "  ✓ nexus-core 是零内部依赖的根节点"
else
    echo "  ✗ nexus-core 依赖了：$(deps_of nexus-core)"
    fail=1
fi

echo ""
echo "── 规则 2：能力层不得依赖 service / api / cli ──"
UPPER="nexus-service nexus-api nexus-cli"
violations=0
for c in $ALL_CRATES; do
    for d in $(deps_of "$c"); do
        for u in $UPPER; do
            if [ "$d" = "$u" ] && [ "$c" != "nexus-service" ] && [ "$c" != "nexus-api" ] && [ "$c" != "nexus-cli" ]; then
                echo "  ✗ $c -> $d（能力层反向依赖上层）"
                violations=$((violations + 1))
            fi
        done
    done
done
if [ "$violations" -eq 0 ]; then
    echo "  ✓ 无能力层反向依赖 service/api/cli"
else
    fail=1
fi

echo ""
echo "── 规则 3：sqlx 只出现在 infra / repo / execution ──"
violations=0
for c in $ALL_CRATES; do
    if grep -q '^sqlx' "crates/$c/Cargo.toml" 2>/dev/null; then
        case "$c" in
            nexus-infra|nexus-repo|nexus-execution) echo "  ✓ $c" ;;
            *) echo "  ✗ $c 不应直接依赖 sqlx"; violations=$((violations + 1)) ;;
        esac
    fi
done
if [ "$violations" -eq 0 ]; then
    echo "  ✓ 数据访问层收敛正确"
else
    fail=1
fi

echo ""
echo "── 规则 4：nexus-infra 不依赖任何上层 crate ──"
violations=0
for d in $(deps_of nexus-infra); do
    case "$d" in
        nexus-core|nexus-config|nexus-observability) ;;
        *) echo "  ✗ nexus-infra -> $d"; violations=$((violations + 1)) ;;
    esac
done
if [ "$violations" -eq 0 ]; then
    echo "  ✓ nexus-infra 只依赖底层 crate"
else
    fail=1
fi

echo ""
echo "── 规则 5：能力层不依赖 nexus-repo（防环，设计文档 §4.1.2）──"
violations=0
for c in nexus-llm nexus-tool nexus-rag nexus-memory nexus-capacity nexus-ingestion \
         nexus-vector nexus-security nexus-config nexus-observability nexus-model; do
    if grep -q '^nexus-repo\.workspace' "crates/$c/Cargo.toml" 2>/dev/null; then
        echo "  ✗ $c 依赖了 nexus-repo（方向反了，应为 repo → 能力层）"
        violations=$((violations + 1))
    fi
done
if [ "$violations" -eq 0 ]; then
    echo "  ✓ 能力层均不依赖 nexus-repo"
else
    fail=1
fi

echo ""
echo "── 规则 6：nexus-model 不含数据库痕迹（设计文档 §4.2 差异 2）──"
violations=0

# 6a. **依赖层**（编译期铁律）—— nexus-model 不得依赖 sqlx。
#
# 这是**最强的**保证：只要 Cargo.toml 里没有 sqlx，
# nexus-model 的代码就**不可能**用上 `#[derive(FromRow)]`。
#
# 为什么必须有这一条：源码扫描是**词法**检查，它拦不住
# 「通过 re-export 或宏间接引入」的情况。依赖层检查没有这个漏洞。
if grep -qE '^sqlx' crates/nexus-model/Cargo.toml 2>/dev/null; then
    echo "  ✗ crates/nexus-model/Cargo.toml 依赖了 sqlx"
    violations=$((violations + 1))
fi

# 6b. **源码层** —— nexus-model 的**代码**（不含注释）里不得出现 sqlx 痕迹。
#
# ⚠️ 必须**排除注释**：文档注释里提到 `FromRow` / `sqlx::` 是**说明性文字**
# （比如「本 crate 不含 FromRow」），不是数据库痕迹。
# 早先的版本用裸 grep 扫描全文，会把这些说明文字误判成违规 ——
# 那样写文档反而会让门禁红。
#
# 剥离方法：删掉每行 `//` 之后的内容（覆盖 `//`、`///`、`//!` 三种前缀），
# 再扫描剩下的**真正的代码**。
#
# 注意：grep 无匹配时返回 1，在 `set -e` 下会中断脚本 —— 必须用 `|| true` 兜住。
if [ -d crates/nexus-model/src ]; then
    hits=$(find crates/nexus-model/src -name '*.rs' -type f -print0 \
        | xargs -0 sed -e 's://.*::' \
        | grep -nE 'FromRow|sqlx::|sqlx_macros' 2>/dev/null || true)
    if [ -n "$hits" ]; then
        echo "$hits" | sed 's|^|  ✗ crates/nexus-model/src（代码部分）: |'
        violations=$((violations + 1))
    fi
fi
if [ "$violations" -eq 0 ]; then
    echo "  ✓ nexus-model 保持纯 DTO"
else
    fail=1
fi

echo ""
echo "── 规则 7：deps_of 与各 crate 的 Cargo.toml 必须一致 ──"
# 为什么这条必需
#
# `deps_of` 是本脚本的**声明式依赖表** —— 规则 1/2/4/5 全部基于它判定。
# 如果它与各 crate 实际的 `Cargo.toml` 漂移了，那些规则就变成
# **在错误的依赖图上做正确的判定** —— 看起来全绿，实际什么都没拦住。
#
# 这不是假设性的风险：§5.3 的架构测试表曾把「`deps_of` ↔ `Cargo.toml`
# 一致性核对」写成「已落地」，而脚本里**并没有**这条规则。
# 文档声称与实现不符 —— 本身就是一种缺陷。
#
# 核对方式：逐 crate 比较「`Cargo.toml` 的 `[dependencies]` 里声明的
# `nexus-*` 内部依赖」与「`deps_of` 声明的内部依赖」。
# **集合相等**才算通过 —— 多一个、少一个都要报出来。
violations=0
for c in $ALL_CRATES; do
    # 实际声明：读 Cargo.toml 的 [dependencies] 段。
    actual=$(awk '
        /^\[/ { in_deps = ($0 == "[dependencies]"); next }
        !in_deps { next }
        $1 ~ /^nexus-[a-z0-9-]+[.]workspace$/ {
            name = $1
            sub(/[.]workspace$/, "", name)
            print name
        }
    ' "crates/$c/Cargo.toml" 2>/dev/null | sort | tr '\n' ' ')

    # `grep -v '^$'` 在无匹配时返回 1 —— 在 `set -e` + `pipefail` 下会中断脚本。
    # `nexus-core` 的依赖表是空的，正好会命中这个分支，所以必须 `|| true` 兜住。
    declared=$(deps_of "$c" | tr ' ' '\n' | grep -v '^$' | sort | tr '\n' ' ' || true)

    if [ "$actual" != "$declared" ]; then
        echo "  ✗ ${c}：依赖声明不一致"
        echo "      Cargo.toml : $actual"
        echo "      deps_of    : $declared"
        violations=$((violations + 1))
    fi
done
if [ "$violations" -eq 0 ]; then
    echo "  ✓ deps_of 与各 crate 的 Cargo.toml 一致"
else
    fail=1
fi

echo ""
echo "── 实际依赖图（cargo tree 校验） ──"
if cargo tree --workspace --prefix none --depth 1 > /dev/null 2>&1; then
    echo "  ✓ cargo tree 可解析，实际图与声明一致（cargo 自身会拒绝环）"
else
    echo "  ✗ cargo tree 解析失败"
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 依赖方向检查全部通过"
else
    echo "✗ 存在违反依赖方向的 crate"
fi
exit "$fail"
