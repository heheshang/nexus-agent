---
id: NEX-42
title: "feat(execution): 落地操作幂等账本与三种 replay_policy"
status: To Do
assignee: []
labels: [execution, idempotency, core]
priority: high
dependencies:
  - NEX-39
  - NEX-41
references:
  - "docs/specs/p6/spec.md §3.8、§2.5"
  - "docs/specs/p6/tasks.md T-006"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.6.3"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.1"
---

## Description

**问题**：「同一路径不重复执行」是持久执行的**核心不变量**。
参考架构 §3.4.1 的 `begin_operation` 有一整套**分支顺序**，
而那个顺序**每一条都有语义**：

- 参数变了 → **冲突**（不复用旧结果）；
- 已成功 → **幂等命中**（返回旧输出）；
- `unknown` → **要求人工核对**（**不重放**）；
- `manual` 策略且执行者丢失 → 标 `unknown`（**不自动重试**）。

**为什么 `manual` 策略是 P6 的必做项**（决策 D14，OQ-3）：
架构文档附录 C 的未验证项里，`UnknownOutcome` 分类标注为
「🟡 **部分验证**（P3 只验证了网关层；『端到端不重复副作用』**需要 P6 的持久执行来验证**）」。
只做 `readonly` 的话，附录 C 这一项**无法在 P6 关闭**。

## 范围边界

### 做

- `nexus-execution/src/operation.rs`：`ReplayPolicy` / `begin_operation` / `complete_operation`
- `execution_operations` / `execution_attempts` 的读写

- `attempt` 上限**作为配置项**
> ⚠️ **持久化落点（决策 D12 · 选项 B）**：引擎侧持久化能力**留在** `nexus-repo`
> （租约 / fencing / 幂等提交 / 事件序号 / 引擎侧行类型与 SQL）。
> 本任务在 `nexus-execution` 里写**执行语义**，落库**经由** `nexus-repo` 的仓储 API，
> **不自己**拼 SQL；「同一个事务」意味着该事务由 `nexus-repo` 侧持有并被两侧共用。

### 不做

| 项 | 理由 |
|---|---|
| 取消时的 `unknown` 化 | 属 NEX-45 |
| 人工核对（`resolve_operation`） | 属 NEX-45 |
| 工具的**授权**判定 | 属 P8（`nexus-tool`） |
| 只做 `readonly` 一种策略 | 决策 D14 —— 那会让架构文档附录 C 无法关闭 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-execution/src/operation.rs` 定义 `ReplayPolicy` enum —— `ReadOnly` / `Idempotent` / `Manual`，**序列化到 DB 用 `&'static str`**（`'readonly'` / `'idempotent'` / `'manual'`）
- [ ] #2 `begin_operation` 在**一个事务**里完成：查既有 operation → 判定 → 创建或重置 → 写 `execution_attempts` 一行
- [ ] #3 既有 operation 的 `args_hash` / `kind` / `replay_policy` **任一**不同 → **冲突**（不复用旧结果）
- [ ] #4 既有 operation `Succeeded` → **幂等命中**，返回旧输出（不重新执行）
- [ ] #5 既有 operation `unknown` → **要求人工核对**，**不重放**
- [ ] #6 既有 operation `Running` 且 `generation` 与当前一致 → **冲突**（并发同节点）
- [ ] #7 `manual` 策略且上次执行者丢失 → 该 operation 与 attempt 标 `unknown` + 追加 `operation_unknown` 事件
- [ ] #8 `attempt` 达上限 → **尝试耗尽**失败；**上限是配置项**，改配置即改上限（**不是**硬编码常数）
- [ ] #9 `attempt` 上限可从 `nexus-config` 的配置栈读取，缺失时用默认值
- [ ] #10 `complete_operation` 在**同一事务**里更新 operation 与 attempt，并追加事件
- [ ] #11 **三种策略各自**有测试 —— 不是只测 `readonly`
- [ ] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

12 条 AC 全部可二元判定 —— 每条都指明「类型 / 分支存在」「返回值可断言」
「落库状态可观测」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.8、§2.5` —— 存在
- `docs/specs/p6/tasks.md T-006` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.6.3` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.1` —— 存在
- `crates/nexus-infra/migrations/5_execution_support.up.sql` —— 存在（`execution_operations` / `execution_attempts`）

### #4 范围有界

**单 crate**（`nexus-execution`）的**一个**新模块 + 测试。
**不新增**依赖方向、**不新增**业务码。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-execution/src/operation.rs` | **新增** |
| `crates/nexus-execution/src/lib.rs` | **修改**（模块声明） |
| `crates/nexus-execution/tests/operation.rs` | **新增** |
| `crates/nexus-config` 的配置项 | **修改**（新增 `attempt` 上限的配置路径 —— 见 #7） |
| `crates/nexus-execution/Cargo.toml` | **不修改**（`nexus-config` 已声明） |

### #6 完成态可描述

调用方对同一个 `path` 连续执行两次：
第一次执行并记账，第二次**命中账本**返回旧输出而不重复执行；
参数变了则**报冲突**；`manual` 策略失败后 run 转 `needs_attention` 而**不是**自动重试。

### #7 无隐形依赖

- 前置：NEX-39（节点执行契约）、NEX-41（driver）—— 均已登记于 `backlog/tasks/`
- `attempt` 上限的配置路径**由本任务新增**（如 `worker.operation_attempt_limit`）——
  **这是已指名的动作**，且必须走 `nexus-config` 的配置栈（不是硬编码）
- `execution_operations` / `execution_attempts` 表**已由 NEX-9 建立**（迁移 5）—— 不需要新迁移
- 集成测试需要真实 PostgreSQL 17 —— P2 的基座**已存在**（NEX-14）

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：三种 `replay_policy` **全做**（`readonly` / `idempotent` / `manual`）。
- **依据**：架构文档附录 C 把「端到端不重复副作用」的验证责任**指派给 P6**；
  「不重复副作用」的**唯一**实现路径就是 `manual` 策略 ——
  标 `unknown`、**不自动重试**、转 `needs_attention` 等人工核对。
  只做 `readonly` 的话，附录 C 这一项**无法在 P6 关闭**。
- **可逆性**：高 —— 补一种策略是增量。
- **影响面**：`crates/nexus-execution/src/operation.rs`。

### 类别：decide-and-proceed

- **决策**：`attempt` 上限是**配置项**，不是硬编码常数。
- **依据**：参考架构 §3.4.6 的取舍说明明写 ——
  「`Attempt >= 3` 是硬编码常量（`operations.go:81`），**Rust 版应做成配置项**」。
- **可逆性**：高 —— 配置项。
- **影响面**：`crates/nexus-execution/src/operation.rs`、`crates/nexus-config`。

### 类别：decide-and-proceed

- **决策**：`ReplayPolicy` 序列化到 DB 用 `&'static str`（`'readonly'` / `'idempotent'` / `'manual'`）。
- **依据**：参考架构 §3.4.1 明写「**序列化到 DB 时用 `&'static str`**，
  与 Go 版 CHECK 约束一致」—— 而迁移 5 的 `execution_operations.replay_policy`
  的默认值就是 `'manual'`，CHECK 约束**已由数据库**下沉。
- **可逆性**：高 —— 序列化形状与既有 CHECK 约束绑定。
- **影响面**：`crates/nexus-execution/src/operation.rs`。
