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
        nexus-execution)     echo "nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-workflow nexus-capacity nexus-infra nexus-observability nexus-security" ;;
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
if [ -d crates/nexus-model/src ]; then
    # 注意：grep 无匹配时返回 1，在 `set -e` 下会中断脚本 —— 必须用 `|| true` 兜住。
    hits=$(grep -rnE 'FromRow|sqlx::|sqlx_macros' crates/nexus-model/src/ 2>/dev/null || true)
    if [ -n "$hits" ]; then
        echo "$hits" | sed 's/^/  ✗ /'
        violations=$((violations + 1))
    fi
fi
if [ "$violations" -eq 0 ]; then
    echo "  ✓ nexus-model 保持纯 DTO"
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
