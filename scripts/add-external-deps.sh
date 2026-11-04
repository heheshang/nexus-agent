#!/usr/bin/env bash
# 为各 crate 补充外部依赖（内部依赖已在 scaffold.sh 生成）。
#
# 幂等：重复运行不会重复追加。
# 注意 1：macOS 自带 bash 3.2 不支持关联数组，这里用「每行一条」的两列列表。
# 注意 2：外部依赖必须插到 [lints] 之前 —— TOML 里追加到文件末尾会落进最后一个
#         表格（[lints]），导致 "invalid type: boolean true, expected a string or map"。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATES="$ROOT/crates"

# 格式：<crate 名>|<空格分隔的外部依赖>
DEP_LINES=(
    "nexus-core|serde serde_json thiserror uuid chrono tokio-util async-trait tracing"
    "nexus-config|serde serde_json serde_yaml_ng toml config dotenvy secrecy chrono"
    "nexus-model|serde serde_json chrono uuid"
    "nexus-observability|tracing tracing-subscriber tracing-opentelemetry opentelemetry opentelemetry_sdk opentelemetry-otlp opentelemetry-semantic-conventions prometheus serde_json"
    "nexus-infra|sqlx redis deadpool-redis bytes tokio futures async-trait tracing secrecy"
    "nexus-security|aes-gcm sha2 hkdf hmac jsonwebtoken rand secrecy zeroize regex hex tracing"
    "nexus-vector|milvus-sdk serde_json async-trait tokio tracing uuid"
    "nexus-llm|reqwest serde_json serde async-trait tokio futures bytes eventsource-stream tokio-util tracing secrecy base64 url chrono"
    "nexus-tool|serde serde_json async-trait tokio tracing anyhow bollard bytes base64 futures tokio-util"
    "nexus-rag|serde_json async-trait tokio tracing futures anyhow jsonschema base64"
    "nexus-ingestion|serde_json async-trait tokio tracing anyhow sha2 hex bytes pdf-extract futures"
    "nexus-capacity|tokio dashmap tracing serde_json async-trait"
    "nexus-workflow|serde serde_json evalexpr async-trait tracing anyhow uuid chrono"
    "nexus-execution|serde serde_json sqlx async-trait tokio tracing uuid chrono tokio-util anyhow"
    "nexus-repo|sqlx serde_json serde async-trait tokio tracing uuid chrono anyhow"
    "nexus-agent|serde serde_json async-trait tokio futures tracing anyhow tokio-util uuid chrono"
    "nexus-memory|serde_json async-trait tokio tracing anyhow uuid chrono"
    "nexus-mcp|rmcp serde serde_json async-trait tokio tracing anyhow tokio-util"
    "nexus-a2a|serde serde_json async-trait tokio tracing anyhow"
    "nexus-service|serde_json async-trait tokio tracing anyhow tokio-util futures"
    "nexus-api|axum tower tower-http hyper hyper-util tokio tokio-stream tokio-util futures async-trait serde_json tracing uuid chrono mime"
)

# 清除先前错误追加到 [lints] 段的外部依赖区块
for line in "${DEP_LINES[@]}"; do
    name="${line%%|*}"
    manifest="$CRATES/$name/Cargo.toml"
    [ -f "$manifest" ] || continue
    if grep -q "# >>> external-deps" "$manifest"; then
        # 删掉从标记行到文件末尾的所有内容（标记行之后全是误追加的外部依赖）
        awk '/# >>> external-deps/{exit} {print}' "$manifest" > "$manifest.tmp"
        mv "$manifest.tmp" "$manifest"
    fi
done

for line in "${DEP_LINES[@]}"; do
    name="${line%%|*}"
    deps="${line#*|}"
    manifest="$CRATES/$name/Cargo.toml"

    if [ ! -f "$manifest" ]; then
        echo "skip  $name（不存在）"
        continue
    fi

    # 构造外部依赖区块到临时文件（awk -v 无法携带换行）
    block_file="$(mktemp)"
    for dep in $deps; do
        echo "$dep.workspace = true" >> "$block_file"
    done

    # 插入到 [lints] 之前；没有 [lints] 则追加到末尾
    if grep -q '^\[lints\]' "$manifest"; then
        awk -v blockfile="$block_file" '
            /^\[lints\]/ && !done {
                while ((getline line < blockfile) > 0) print line
                close(blockfile)
                done = 1
            }
            { print }
        ' "$manifest" > "$manifest.tmp"
    else
        { echo ""; echo "[dependencies]"; cat "$block_file"; } >> "$manifest"
    fi
    mv "$manifest.tmp" "$manifest"
    rm -f "$block_file"

    echo "deps  $name"
done

echo ""
echo "外部依赖注入完成"
