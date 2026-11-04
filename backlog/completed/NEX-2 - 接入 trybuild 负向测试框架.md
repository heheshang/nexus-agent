---
id: NEX-2
title: "chore(core): 接入 trybuild 负向测试框架"
status: Done
assignee: []
labels: [core, testing, p0]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.2"
---

## Description

架构文档 §6.2 把「负向测试框架（`trybuild` 接入，验证 §2.1.3）」列为 P0 任务，目前尚未完成。

`trybuild` 只存在于 `examples/demo-cancel-typestate`，主 workspace 的 `Cargo.toml` 没有该依赖，`crates/nexus-core/tests/` 下也没有 `compile_fail/` 骨架。

主 workspace 需要的是**框架与骨架**（不是一个可运行的负向测试 —— `nexus-core` 的取消语义类型属于 P1）。

可参考的现成实现：`examples/demo-cancel-typestate/`（含 `tests/compile_fail.rs` 与 `tests/compile_fail/*.rs` + `.stderr` 快照）。

## Acceptance Criteria

- [ ] #1 根 `Cargo.toml` 的 `workspace.dependencies` 增加 `trybuild`，`crates/nexus-core/Cargo.toml` 以 `dev-dependencies` 引用（版本号不硬编码）
- [ ] #2 `crates/nexus-core/tests/` 下存在 `compile_fail/` 骨架目录与 `compile_fail.rs` 测试入口
- [ ] #3 `compile_fail.rs` 的 trybuild 编译失败用例当前**全部通过**（即断言的失败确实发生）—— P1 未落地类型前，用例集为空或使用占位
- [ ] #4 `cargo test --workspace --locked` 通过，且 `Cargo.lock` 已同步入库
- [ ] #5 `bash scripts/check-lock.sh` 通过（成员 crate 未硬编码版本号）
- [ ] #6 `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- [ ] #7 CI 六道门禁全绿（含 `scripts/check-naming.sh`）
