---
id: NEX-43
title: "feat(execution): 落地检查点版本三元组"
status: To Do
assignee: []
labels: [execution, checkpoint, core]
priority: medium
dependencies:
  - NEX-37
references:
  - "docs/specs/p6/spec.md §3.9"
  - "docs/specs/p6/tasks.md T-007"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.3"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.2"
---

## Description

**问题**：检查点是续跑的「断点」。**没有版本绑定的检查点是一颗定时炸弹** ——
用新引擎读旧检查点（或反过来），状态会被**错误地**解释。

参考架构 §3.4.3 的关键设计是「**为什么要写两次版本**」：

1. **既有 run 的版本** —— 防「用新引擎读旧检查点」；
2. **当前进程的版本** —— 防「用旧引擎写新检查点」。

**修法**：检查点绑定三元组 `(definition_hash, `engine_version`, `graph_schema)`；
任一不符即**拒绝**读写。

## 范围边界

### 做

- `nexus-execution/src/checkpoint.rs`：检查点的读写
- 三元组比对（既有 run 的版本 + 当前进程常量）

> ⚠️ **持久化落点（决策 D12 · 选项 B）**：引擎侧持久化能力**留在** `nexus-repo`
> （租约 / fencing / 幂等提交 / 事件序号 / 引擎侧行类型与 SQL）。
> 本任务在 `nexus-execution` 里写**执行语义**，落库**经由** `nexus-repo` 的仓储 API，
> **不自己**拼 SQL；「同一个事务」意味着该事务由 `nexus-repo` 侧持有并被两侧共用。

### 不做

| 项 | 理由 |
|---|---|
| 检查点的**负载压缩 / 序列化格式** | 参考架构未定稿；P6 用**原始字节**（`BYTEA` 列已存在） |
| 状态迁移（`state_version` 的业务语义） | 参考架构 §3.4.3 未展开；P6 只做**版本绑定** |
| 应用层的负载大小上限 | 迁移 5 的 CHECK 约束**已下沉到数据库** —— 不重复 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-execution/src/checkpoint.rs` 定义检查点的读写
- [ ] #2 检查点绑定**三元组** `(definition_hash, `engine_version`, `graph_schema)` —— 三个都存
- [ ] #3 读：比对**既有 run 的版本** 与 **当前进程常量**；任一不同 → 版本不匹配（`RunConflict`）
- [ ] #4 写：`INSERT ... ON CONFLICT DO UPDATE` **带三元组条件**；影响行数 ≠ 1 → 版本不匹配
- [ ] #5 同一 `run_id` + `checkpoint_id` **覆盖**写（复合主键），不是插入重复行
- [ ] #6 负载大小上限**下沉到数据库**（`octet_length(payload) <= 16777216`）—— 应用层**不重复**这个约束
- [ ] #7 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

7 条 AC 全部可二元判定 —— 每条都指明「函数存在」「读写结果可观测」
「影响行数可断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.9` —— 存在
- `docs/specs/p6/tasks.md T-007` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.3` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.2` —— 存在
- `crates/nexus-infra/migrations/5_execution_support.up.sql` —— 存在（`execution_checkpoints`）

### #4 范围有界

**单 crate**（`nexus-execution`）的**一个**新模块 + 测试。
**不新增**依赖方向、**不新增**业务码、**不需要**新迁移（表已存在）。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-execution/src/checkpoint.rs` | **新增** |
| `crates/nexus-execution/src/lib.rs` | **修改**（模块声明） |
| `crates/nexus-execution/tests/checkpoint.rs` | **新增** |
| `crates/nexus-execution/Cargo.toml` | **不修改** |

### #6 完成态可描述

调用方能存一个检查点、读回同一个检查点；
当定义的哈希 / 引擎版本 / 图 schema 变了，读写都被**拒绝**并返回 `RunConflict`；
重复写同一个检查点**不会**产生第二行。

### #7 无隐形依赖

- 前置：NEX-37（引擎侧持久化归属落地，已登记于 `backlog/completed/NEX-37 - ....md`）
- `execution_checkpoints` 表**已由 NEX-9 建立**（迁移 5）—— 不需要新迁移
- 负载上限的 CHECK 约束**已在数据库里** —— 应用层不重复实现
- 集成测试需要真实 PostgreSQL 17 —— P2 的基座**已存在**（NEX-14）

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：负载大小上限**只**由数据库的 CHECK 约束实现，应用层**不重复**。
- **依据**：架构文档 §3.9.2「约束下沉到数据库」的既定做法；
  迁移 5 里 `CONSTRAINT execution_checkpoints_payload_check CHECK (octet_length(payload) <= 16777216)` **已经存在**。
  应用层再写一遍 = 两份定义 = 漂移的入口（NEX-12 在围栏条件上踩过同一个坑）。
- **可逆性**：高 —— 不动作。
- **影响面**：`crates/nexus-execution/src/checkpoint.rs`。
