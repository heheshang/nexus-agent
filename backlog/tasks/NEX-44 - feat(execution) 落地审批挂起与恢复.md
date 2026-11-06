---
id: NEX-44
title: "feat(execution): 落地审批挂起与恢复"
status: To Do
assignee: []
labels: [execution, approval, core]
priority: high
dependencies:
  - NEX-40
  - NEX-41
references:
  - "docs/specs/p6/spec.md §3.6、§3.10"
  - "docs/specs/p6/tasks.md T-008"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.6、§3.5.3"
---

## Description

**问题**：这是 P6 完成判据的**第三项** —— 「**审批恢复**有测试」。

参考架构 §2.6 的语义表里有四条**各自独立**的规则，每一条都有代价：

| 场景 | 语义 |
|---|---|
| 重复决策 | 相同决策重复提交**幂等**返回；冲突决策 → 冲突 |
| 过期 | `expires_at > clock_timestamp()` 才允许写入决策 |
| 参数变化 | 挂起时发现已有审批的 `args_hash` / `path` 与本次不同 → 版本不匹配 |
| 恢复二次校验 | `decision.args_hash != proposal.args_hash` → 报错 |

**为什么「幂等」与「冲突」要分开**：重复点击「批准」是**正常**的用户行为，
不是错误 —— 报错会让前端必须做去重。不同决策才是真冲突。

**修法**：落地 `suspend` / `decide` / 恢复路径。

## 范围边界

### 做

- `nexus-execution/src/approval.rs`：`suspend` / `decide` / 恢复路径
- `execution_approvals` 表的读写

> ⚠️ **持久化落点（决策 D12 · 选项 B）**：引擎侧持久化能力**留在** `nexus-repo`
> （租约 / fencing / 幂等提交 / 事件序号 / 引擎侧行类型与 SQL）。
> 本任务在 `nexus-execution` 里写**执行语义**，落库**经由** `nexus-repo` 的仓储 API，
> **不自己**拼 SQL；「同一个事务」意味着该事务由 `nexus-repo` 侧持有并被两侧共用。

### 不做

| 项 | 理由 |
|---|---|
| 审批的 **HTTP 端点** | 决策 D17（OQ-6）—— 属 `nexus-service`（7 行空壳） |
| `nexus-service` 的接线 | 同上 |
| 中断提案的生成 | 属 NEX-40 |
| 危险工具的授权策略 | 属 P8（`nexus-tool`） |

## Acceptance Criteria

- [ ] #1 `crates/nexus-execution/src/approval.rs` 实现 `suspend` —— 写 `execution_approvals`（`decision = 'pending'`）并把 run 置 `waiting_approval`
- [ ] #2 `decide` 校验 `args_hash` 一致且状态仍为 `pending`
- [ ] #3 `approved` → run 置 `queued`（**可续跑**）
- [ ] #4 `rejected` → run 置 `canceled` 且 `generation + 1`
- [ ] #5 **重复的同决策** → **幂等**返回，不报错（不是冲突）
- [ ] #6 **冲突的决策**（同 interrupt 不同决策） → 冲突错误
- [ ] #7 **过期**的审批（`expires_at <= clock_timestamp()`） → 拒绝写入决策（冲突错误）
- [ ] #8 挂起时发现已有审批的 `args_hash` / `path` 与本次不同 → 版本不匹配（`RunConflict`）
- [ ] #9 恢复时二次校验 `decision.args_hash == proposal.args_hash` —— 不一致**拒绝恢复**
- [ ] #10 审批挂起与恢复**各自**有集成测试（需要真实 PostgreSQL 17）
- [ ] #11 审批恢复的集成测试**自带种子数据**（`users` / `workflows` / `workflow_versions`）—— **不依赖** `workflows` 的写入方（决策 D13）
- [ ] #12 P6 **不交付**审批的 HTTP 端点（决策 D17）—— `crates/nexus-execution` 源码**不含** axum / `Router` 引用
- [ ] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

13 条 AC 全部可二元判定 —— 每条都指明「函数存在」「落库状态可观测」
「错误可断言」「结构层源码断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.6、§3.10` —— 存在
- `docs/specs/p6/tasks.md T-008` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.6、§3.5.3` —— 存在
- `crates/nexus-infra/migrations/5_execution_support.up.sql` —— 存在（`execution_approvals`）

### #4 范围有界

**单 crate**（`nexus-execution`）的**一个**新模块 + 测试。
**不新增**依赖方向、**不新增**业务码、**不需要**新迁移（表已存在）。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-execution/src/approval.rs` | **新增** |
| `crates/nexus-execution/src/lib.rs` | **修改**（模块声明） |
| `crates/nexus-execution/tests/approval.rs` | **新增** |
| `crates/nexus-execution/tests/architecture.rs` | **修改**（新增「无 HTTP 端点」断言） |
| `crates/nexus-execution/Cargo.toml` | **不修改** |

### #6 完成态可描述

一次运行挂起等待人工审批时，操作者可以在库里看到一条 `pending` 审批；
批准后 run 回到 `queued` 并能续跑，拒绝后 run 变成 `canceled`；
重复点「批准」不会报错，过期的审批与参数变过的审批都会被**拒绝**。

### #7 无隐形依赖

- 前置：NEX-40（中断提案与审批指纹）、NEX-41（driver）—— 均已登记于 `backlog/tasks/`
- `execution_approvals` 表**已由 NEX-9 建立**（迁移 5）—— 不需要新迁移
- 审批端点**已明确划出范围**（决策 D17）—— 不是隐形依赖
- `workflows` 的写入方**已明确划出范围**（决策 D13）—— 集成测试自带种子数据
- 集成测试需要真实 PostgreSQL 17 —— P2 的基座**已存在**（NEX-14）

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：重复的**同**决策是**幂等**的，重复的**不同**决策才是冲突。
- **依据**：参考架构 §2.6 的语义表原文 ——
  「重复决策：相同决策重复提交幂等返回 nil；冲突决策 → `ErrConflict`」。
  重复点击「批准」是**正常**用户行为，报错会让前端必须做去重。
- **可逆性**：高 —— 语义分支。
- **影响面**：`crates/nexus-execution/src/approval.rs`。

### 类别：decide-and-proceed

- **决策**：审批的**过期**判定用 `clock_timestamp()`。
- **依据**：与围栏条件**同一条纪律**（架构文档 §2.5.4）——
  `now()` 是事务开始时间，长事务里会把「已过期」误判成「未过期」。
- **可逆性**：高 —— 一处 SQL 表达式。
- **影响面**：`crates/nexus-execution/src/approval.rs`。

### 类别：decide-and-proceed

- **决策**：审批**不交付** HTTP 端点，P6 **不接线** `nexus-service`。
- **依据**：参考实现的审批端点属 service 层；`nexus-service` 是 7 行空壳；
  P5 的决策 D9 已定「业务编排属 `nexus-service`」。
  P6 交付**仓储语义与恢复语义** —— 那才是「审批恢复有测试」的判据。
- **可逆性**：高 —— 端点是**注册**到既有路由骨架的增量。
- **影响面**：无（不交付即无影响）；审批端点登记为 F5。
