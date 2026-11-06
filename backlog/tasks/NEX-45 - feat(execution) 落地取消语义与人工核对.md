---
id: NEX-45
title: "feat(execution): 落地取消语义与人工核对"
status: To Do
assignee: []
labels: [execution, cancellation, reconciliation]
priority: high
dependencies:
  - NEX-42
references:
  - "docs/specs/p6/spec.md §3.11、§2.4、§2.5、§2.6"
  - "docs/specs/p6/tasks.md T-009"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.1、§2.6.3、附录 C"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.4、§3.4.5"
---

## Description

**问题**：参考架构 §3.4.4 抄了 Go 版的注释原文：

> Cancel cannot undo a remote call already sent. Preserve uncertainty before
> fencing the executor so operators can inspect what happened.

**「先保留不确定性，再 fence 执行者」** —— 这两件事的**顺序**是语义的一部分。
调换顺序 = 取消后**查不清**「到底发生了什么」。

而 `needs_attention` + `resolve_operation` 是 P6 **唯一**能让架构文档附录 C 的
`UnknownOutcome` 项**关闭**的地方。

**修法**：落地取消路径（`unknown` 化 / `abandoned` / run 取消）与人工核对
（`resolve_operation`，**必须**带 evidence）。

## 范围边界

### 做

- `nexus-execution/src/cancel.rs`：`cancel`（在途操作的分类处理 + run 取消）
- `nexus-execution/src/reconcile.rs`：`resolve_operation`（人工核对，带 evidence）
- `operation_unknown` / `operation_reconciled` 事件

> ⚠️ **持久化落点（决策 D12 · 选项 B）**：引擎侧持久化能力**留在** `nexus-repo`
> （租约 / fencing / 幂等提交 / 事件序号 / 引擎侧行类型与 SQL）。
> 本任务在 `nexus-execution` 里写**执行语义**，落库**经由** `nexus-repo` 的仓储 API，
> **不自己**拼 SQL；「同一个事务」意味着该事务由 `nexus-repo` 侧持有并被两侧共用。

### 不做

| 项 | 理由 |
|---|---|
| 取消的 HTTP 端点 | 属 `nexus-service`（决策 D17） |
| `needs_attention` 的运维界面 | 属 P9 / 前端 |
| `unknown` 操作的自动重放 | **绝不做** —— 参考架构 §3.4.5 明写「never a blind automatic replay」 |

## Acceptance Criteria

- [ ] #1 `cancel` 在**一个事务**里：扫描 `status = 'running'` 的 operation → 分类更新 → 更新 run
- [ ] #2 在途 `manual` 操作 → 标 `unknown` + 追加 `operation_unknown` 事件（reason = `canceled_in_flight`）
- [ ] #3 在途**其他**策略的操作 → 标 `abandoned`
- [ ] #4 run 本身 → `status = canceled`、`generation + 1`、清租约
- [ ] #5 **顺序不可调换** —— 先保留不确定性，**再** fence 执行者（测试断言事件写入与 run 更新的**先后**可见）
- [ ] #6 `resolve_operation` 的前置条件：run 处于 `needs_attention`、operation 处于 `unknown`、**且** `args_hash` 匹配
- [ ] #7 核对后 operation → `succeeded`，run → `queued` 且 `generation + 1`
- [ ] #8 核对追加 `operation_reconciled` 事件，含 `decided_by` 与 `evidence`
- [ ] #9 核对**绝不**盲自动重放 —— 无 `evidence` 的核对被**拒绝**
- [ ] #10 `UnknownOutcome` 的端到端语义有测试 —— 副作用**不重复**执行（架构文档附录 C 的未验证项因此可关闭）
- [ ] #11 取消**不映射**为 `ErrorCode::Internal`（取消不是失败）
- [ ] #12 `needs_attention` **不合并**进 `failed` —— 落库状态不同
- [ ] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

13 条 AC 全部可二元判定 —— 每条都指明「函数存在」「落库状态可观测」
「事件内容可断言」「错误码可断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.11、§2.4、§2.5、§2.6` —— 存在
- `docs/specs/p6/tasks.md T-009` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.1、§2.6.3、附录 C` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.4、§3.4.5` —— 存在

### #4 范围有界

**单 crate**（`nexus-execution`）的**两个**新模块 + 测试。
**不新增**依赖方向、**不新增**业务码、**不需要**新迁移（表已存在）。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-execution/src/cancel.rs` | **新增** |
| `crates/nexus-execution/src/reconcile.rs` | **新增** |
| `crates/nexus-execution/src/lib.rs` | **修改**（模块声明） |
| `crates/nexus-execution/tests/cancel.rs` | **新增** |

### #6 完成态可描述

操作者在 run 执行中取消它时：
在途的 `manual` 操作在库里是 `unknown`（**不是** `abandoned`），
有一条 `operation_unknown` 事件说明原因；
之后操作者带着证据做人工核对，run 回到 `queued` 并留下 `operation_reconciled` 记录。

### #7 无隐形依赖

- 前置：NEX-42（操作幂等账本 —— `unknown` 化与 `resolve_operation` 依赖它）——
  已登记于 `backlog/tasks/NEX-42 - ....md`
- `execution_operations` / `execution_attempts` 表**已由 NEX-9 建立** —— 不需要新迁移
- `operation_unknown` / `operation_reconciled` 事件走 `execution_events`（表已存在）
- 集成测试需要真实 PostgreSQL 17 —— P2 的基座**已存在**（NEX-14）

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：取消时**先**把在途 `manual` 操作标 `unknown` 并追加事件，**再**更新 run 并 fence。
- **依据**：参考架构 §3.4.4 抄的 Go 版注释原文 ——
  「Cancel cannot undo a remote call already sent. Preserve uncertainty before
  fencing the executor so operators can inspect what happened.」
  顺序是**语义**的一部分，不是实现细节。
- **可逆性**：中 —— 事务内的顺序变更要改测试。
- **影响面**：`crates/nexus-execution/src/cancel.rs`。

### 类别：decide-and-proceed

- **决策**：人工核对**必须**带 `evidence`，无 evidence 的核对被**拒绝**。
- **依据**：参考架构 §3.4.5 的注释原文 ——
  「ResolveOperation records an explicit owner's reconciliation,
  **never a blind automatic replay**. Evidence remains in the reliable audit event.」
  没有证据的核对 = 盲重放的另一种写法。
- **可逆性**：中 —— API 形状。
- **影响面**：`crates/nexus-execution/src/reconcile.rs`。

### 类别：decide-and-proceed

- **决策**：`needs_attention` **不合并**进 `failed`。
- **依据**：参考架构 Phase 9 的已知坑 #1 明写「**绝不能**把 `needs_attention` 合并进 `failed`」。
  `failed` 是「**确定**失败」，`needs_attention` 是「**结果未知**，等人工核对」——
  合并后运维看不出哪些 run 需要人管。
- **可逆性**：中 —— 状态机与落库值。
- **影响面**：`crates/nexus-execution/src/cancel.rs`、`crates/nexus-execution/src/reconcile.rs`。
