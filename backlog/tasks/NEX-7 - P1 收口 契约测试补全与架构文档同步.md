---
id: NEX-7
title: "test(core): P1 收口 —— 契约测试补全与架构文档同步"
status: To Do
assignee: []
labels: [core, testing, docs, p1]
priority: medium
dependencies:
  - NEX-4
  - NEX-5
  - NEX-6
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.3"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.2"
---

## Description

P1 的收口任务：把 §5.2 / §5.3 的验证策略落到可执行断言，并同步架构文档中因 P1 落地而失效的表述。

**P1 的完成判据是「负向测试通过」**（架构文档 §6.1）—— 由 NEX-4 的 AC #9 承担，本任务确保它**真的**通过（而不是被跳过）。

### §5.3 需补充的架构测试

架构文档 §5.3 列了一张「设计文档落地后需补充」的表：

| 测试 | 检验的设计条款 | 状态 |
|---|---|---|
| `workflow_runs` 只在 `nexus-execution` 出现 | §4.2 差异 1 | P2 才有 `workflow_runs` —— 本任务**不**做 |
| `nexus-model` 不含 `FromRow` / `sqlx::Type` | §4.2 差异 2 | ✅ 已有 |
| `nexus-core` 零内部依赖 | §4.4 规则 2 | ✅ 已有 |
| 能力层不依赖 `nexus-repo` | §4.1.2（防环） | ✅ 已有 |
| `nexus-repo` 只依赖能力层（单向） | §4.1.2 | P2 才有 —— 本任务**不**做 |

### §4.2 需要同步的表格行

架构文档 §4.2 的对照表有一行：

```
| `nexus-core` 子模块 | 类型状态、事件、错误 | **空**（待 P1） | ⬜ |
```

P1 落地后这行**失效** —— 必须更新，否则文档与实现不符。

### §5.5 需要同步的表述

架构文档 §5.5 说「P1 直接把这个 demo 的形状搬进 `nexus-core`」—— 搬完后这句话需要改成已完成状态，并说明 `examples/` 的 demo **保留作为回归基准**（不删除）。

## Acceptance Criteria

- [ ] #1 `crates/nexus-core/tests/compile_fail.rs` 的负向用例**实际运行**（不是被跳过），且 `.stderr` 快照与当前工具链一致
- [ ] #2 `nexus-core` 有一个**跨模块的契约测试**文件，把 ID 类型、事件名清单、错误码表三份契约放在一起（便于一眼看出契约是否齐备）
- [ ] #3 `nexus-core` 的公共 API 有 `Debug` 实现（workspace lint `missing_debug_implementations` 通过）
- [ ] #4 架构文档 §4.2 对照表中 `nexus-core` 子模块那行**已更新**为 P1 落地后的实际状态
- [ ] #5 架构文档 §5.2 的「P1 直接把 demo 形状搬进 `nexus-core`」表述**已更新**，并说明 `examples/` 的 demo 保留作为回归基准
- [ ] #6 架构文档 §6.1 的 P1 行**完成判据**标注为已达成（负向测试通过）
- [ ] #7 `examples/` 下的 12 个 demo **一个都没删**（demo 是回归基准，不是临时代码）
- [ ] #8 `cd examples && cargo test --workspace` 仍然全绿
- [ ] #9 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #10 CI 六道门禁全绿（含 `scripts/check-naming.sh`）

## 备注

- 本任务**不改**任何契约（码值、事件名、ID 底层类型）。若发现契约与实现不符，是**实现**错了，不是契约错了 —— 按 §6.3 的纪律，回退到任务层修正实现，不在收口阶段改契约。
- 文档同步属于 `advisory` 级门禁（只记录不拦截），但本任务把它作为 AC —— 因为契约文档与实现不符会直接误导后续任务。
