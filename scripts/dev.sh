#!/usr/bin/env bash
# 本地开发常用命令的快捷入口。
#
# 用法：bash scripts/dev.sh <命令>
#   check    编译检查（最常用）
#   fmt      格式化
#   lint     clippy
#   test     跑测试
#   lock     更新依赖锁定
#   audit    检查依赖树纪律（lock + deps）
#   build    release 构建
#   run      以 debug 模式运行 CLI
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cmd="${1:-check}"
shift || true

case "$cmd" in
    check)  cargo check --workspace --locked "$@" ;;
    fmt)    cargo fmt --all "$@" ;;
    lint)   cargo clippy --workspace --all-targets --locked -- -D warnings "$@" ;;
    test)   cargo test --workspace --locked "$@" ;;
    lock)   cargo update --workspace "$@" ;;
    audit)  bash scripts/check-lock.sh && bash scripts/check-deps.sh ;;
    build)  cargo build --workspace --release --locked "$@" ;;
    run)    cargo run -p nexus-cli -- "$@" ;;
    *)      echo "未知命令：$cmd"; exit 1 ;;
esac
