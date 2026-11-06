---
id: NEX-41
title: "feat(execution): 落地执行 driver 与版本三元组校验"
status: To Do
assignee: []
labels: [execution, driver, core]
priority: high
dependencies:
  - NEX-37
references:
  - "docs/specs/p6/spec.md §3.7、§2.2"
  - "docs/specs/p6/tasks.md T-005"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.4"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.5、§3.4.3"
---

## Description

**问题**：driver 是「领取 → 校验 → 编译 → 执行 → 终态」的**骨架**。
没有它，持久执行就只是「一堆能写的表」。

**最关键的一条**：driver 在**编译图之前**必须校验**版本三元组**
（`definition_hash` / `engine_version` / `graph_schema`）。
参考架构 §3.4.3 的原话是「**版本不兼容 → 需要迁移路径**」——
不是「重跑一遍」。

**为什么围栏写必须与业务写同事务**（参考架构 Phase 9 必抄不变量 #9）：
围栏检查与业务更新之间如果隔着事务边界，就有窗口 ——
检查通过后、写入前，租约可能已被别人抢走。僵尸 worker 就能写进去。

## 范围边界

### 做

- `nexus-execution/src/driver.rs`：`Driver::execute` 的骨架
- 版本三元组校验（读既有 run 的版本 + 当前进程常量）
- run 状态机的终态集合与转换
- `workflow_runs` 产品视图的**独占写入**（架构文档 §4.2 差异 1）

> ⚠️ **持久化落点（决策 D12 · 选项 B）**：引擎侧持久化能力**留在** `nexus-repo`
> （租约 / fencing / 幂等提交 / 事件序号 / 引擎侧行类型与 SQL）。
> 本任务在 `nexus-execution` 里写**执行语义**，落库**经由** `nexus-repo` 的仓储 API，
> **不自己**拼 SQL；「同一个事务」意味着该事务由 `nexus-repo` 侧持有并被两侧共用。

### 不做

| 项 | 理由 |
|---|---|
| 操作幂等账本（`begin_operation`） | 属 NEX-42 |
| 检查点读写 | 属 NEX-43 |
| 审批挂起与恢复 | 属 NEX-44 |
| 取消与人工核对 | 属 NEX-45 |
| `nexus-service` 的接线 | `nexus-service` 是 7 行空壳；决策 D17 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-execution/src/driver.rs` 定义 `Driver::execute` —— 领取后**先校验**版本三元组，再编译，再执行
- [ ] #2 版本三元组 = `(definition_hash, `engine_version`, `graph_schema)`；**三者任一**不同 → **拒绝续跑**
- [ ] #3 版本不匹配映射到 `ErrorCode::RunConflict`（30007，终止型）—— **不新增**业务码
- [ ] #4 围栏写与业务写在**同一个事务**里 —— 围栏检查与业务更新之间**不跨**事务边界（参考架构 Phase 9 必抄不变量 #9）
- [ ] #5 run 状态机的终态集合**全部**存在：`succeeded` / `failed` / `needs_attention` / `waiting_approval` / `canceled` / `retry_wait`
- [ ] #6 `needs_attention` **不合并**进 `failed` —— 两者的**落库状态**不同（行为层断言查落库值，不是查枚举定义）
- [ ] #7 `nexus-execution` **不依赖** `nexus-service` / `nexus-api` / `nexus-cli`（结构层断言）
- [ ] #8 `nexus-execution` 是 `workflow_runs` / `workflow_step_runs` 的**唯一写入者** —— 结构层断言这两张表的**写操作**（`INSERT` / `UPDATE` / `DELETE`）**只允许**出现在 `nexus-execution`（架构文档 §4.2 差异 1 · 2026-11-06 修订；判定手段由「类型归属」改为「写操作归属」，**强度不降**）
- [ ] #9 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

9 条 AC 全部可二元判定 —— 每条都指明「函数存在」「返回值 / 错误码可断言」
「结构层断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.7、§2.2` —— 存在
- `docs/specs/p6/tasks.md T-005` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.4` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.5、§3.4.3` —— 存在

### #4 范围有界

**单 crate**（`nexus-execution`）的**一个**新模块 + 测试。
**不新增**依赖方向、**不新增**业务码、**不改**错误码值。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-execution/src/driver.rs` | **新增** |
| `crates/nexus-execution/src/lib.rs` | **修改**（模块声明 + crate 级文档） |
| `crates/nexus-execution/tests/driver.rs` | **新增** |
| `crates/nexus-execution/tests/architecture.rs` | **新增**（结构层断言） |
| `crates/nexus-execution/Cargo.toml` | **不修改** |

### #6 完成态可描述

调用方把一个已领取租约的 run 交给 driver，driver 能：
校验版本 → 编译图 → 逐节点执行 → 落到明确的终态；
版本不符时**拒绝**并返回 `RunConflict`；
`needs_attention` 的 run 在库里**可区分**于 `failed`。

### #7 无隐形依赖

- 前置：NEX-37（引擎侧持久化归属落地，已登记于 `backlog/completed/NEX-37 - ....md`）
- 图编译与节点执行属 NEX-38 / NEX-39 —— driver 以 **trait 接缝**调用它们，
  **不依赖**其具体实现细节（与 P5 的决策 D10 同一条纪律）
- 集成测试需要真实 PostgreSQL 17 —— P2 的基座**已存在**（NEX-14）
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：版本三元组**任一**不同 → **拒绝续跑**，映射到 `RunConflict`（30007）。
- **依据**：架构文档 §6.3 已定稿 42 个业务码，**没有**「版本不匹配」专用码；
  新增码 = **前后端契约**的破坏性变更，不属于 P6 的裁量范围。
  这几类情形的共同点是「当前操作与已固化的记录冲突」—— `RunConflict` 语义吻合，
  且它已是**终止型**（重试无意义）。
- **可逆性**：中 —— 改码是破坏性变更（需前后端同步）。
- **影响面**：`crates/nexus-execution/src/driver.rs`。

### 类别：decide-and-proceed

- **决策**：driver 以 **trait 接缝**调用图编译与节点执行，**不依赖**具体实现。
- **依据**：与 P5 的决策 D10（`IdentityExtractor` trait 接缝）、P4 的 D4 同一条纪律 ——
  **消费方定义 trait**，让 `nexus-execution` 不被 `nexus-workflow` 的实现细节绑死。
- **可逆性**：高 —— 接口。
- **影响面**：`crates/nexus-execution/src/driver.rs`。
