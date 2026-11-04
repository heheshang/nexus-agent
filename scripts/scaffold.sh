#!/usr/bin/env bash
# 生成 nexus-agent workspace 的 crate 骨架。
#
# 幂等：已存在的 crate 目录会被跳过（保留其中的实现代码）。
# 用法：bash scripts/scaffold.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATES="$ROOT/crates"

# crate 名 -> 依赖的内部 crate（空格分隔）
# 依赖方向铁律（见 docs/architecture/2026-11-04-...-rust-reference-architecture.md §0.3）：
#   cli → api → service → agent/workflow/execution → repo → infra
#   能力层（llm/tool/rag/memory/capacity）不得反向依赖 service/api
declare -a CRATE_DEPS=(
    "nexus-core:"
    "nexus-config:nexus-core"
    "nexus-model:nexus-core"
    "nexus-observability:nexus-core"
    "nexus-infra:nexus-core nexus-config nexus-observability"
    "nexus-security:nexus-core nexus-config nexus-observability"
    "nexus-vector:nexus-core nexus-config nexus-infra nexus-observability"
    "nexus-llm:nexus-core nexus-config nexus-model nexus-observability nexus-security"
    "nexus-tool:nexus-core nexus-config nexus-model nexus-llm nexus-observability nexus-security"
    "nexus-rag:nexus-core nexus-config nexus-model nexus-llm nexus-observability nexus-security nexus-vector"
    "nexus-ingestion:nexus-core nexus-config nexus-model nexus-infra nexus-observability"
    "nexus-capacity:nexus-core nexus-config nexus-model nexus-observability"
    "nexus-workflow:nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-rag nexus-observability"
    "nexus-execution:nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-workflow nexus-capacity nexus-infra nexus-observability nexus-security"
    "nexus-repo:nexus-core nexus-config nexus-model nexus-infra nexus-observability nexus-security nexus-capacity nexus-rag nexus-ingestion nexus-vector"
    "nexus-agent:nexus-core nexus-config nexus-model nexus-llm nexus-tool nexus-observability nexus-security"
    "nexus-memory:nexus-core nexus-config nexus-model nexus-llm nexus-observability nexus-security nexus-vector nexus-ingestion"
    "nexus-mcp:nexus-core nexus-config nexus-model nexus-security nexus-observability"
    "nexus-a2a:nexus-core nexus-config nexus-model nexus-security nexus-observability nexus-mcp"
    "nexus-service:nexus-core nexus-config nexus-model nexus-observability nexus-infra nexus-security nexus-llm nexus-tool nexus-rag nexus-memory nexus-capacity nexus-workflow nexus-execution nexus-repo nexus-agent nexus-ingestion nexus-mcp nexus-a2a"
    "nexus-api:nexus-core nexus-config nexus-model nexus-observability nexus-security nexus-service nexus-agent nexus-execution nexus-llm"
)

for entry in "${CRATE_DEPS[@]}"; do
    name="${entry%%:*}"
    deps="${entry#*:}"
    dir="$CRATES/$name"

    if [ -d "$dir" ]; then
        echo "skip  $name (已存在)"
        continue
    fi

    mkdir -p "$dir/src"

    # 生成 Cargo.toml
    {
        echo "[package]"
        echo "name = \"$name\""
        echo "version.workspace = true"
        echo "edition.workspace = true"
        echo "rust-version.workspace = true"
        echo "license.workspace = true"
        echo "publish.workspace = true"
        echo ""
        echo "[dependencies]"
        for dep in $deps; do
            echo "$dep.workspace = true"
        done
        echo ""
        echo "[lints]"
        echo "workspace = true"
    } > "$dir/Cargo.toml"

    # 生成 src/lib.rs（除 CLI 外全部是库）
    if [ "$name" = "nexus-cli" ]; then
        mkdir -p "$dir/src/bin"
    else
        cat > "$dir/src/lib.rs" <<EOF
//! $name
//!
//! TODO: 补齐 crate 级文档，说明「本 crate 负责什么 / 不负责什么」。
//! 参见 docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
EOF
    fi

    echo "create $name"
done

echo ""
echo "骨架生成完成：$(ls -1 "$CRATES" | wc -l | tr -d ' ') 个 crate"
